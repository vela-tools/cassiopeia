use cassiopeia_mapping::{
    attribute::{
        Attribute,
        declaration_trail::{DeclarationTrail, Nesting},
        instance::AttributeInstance,
    },
    error::{MappingError, Result},
    mapping::Mapping,
    scope::{CompiledScope, CompiledScopeTemplate, Scope},
    source_reads::SourceReads,
    template::{CompiledTemplate, TemplateSource, runner::TemplateRunner},
    template_location::TemplateLocation,
    template_site::TemplateSite,
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf, scope::ScopeBuf};
use serde_json::Value;
use std::{path::Path, sync::Arc};

/// Compiles a mapping's raw template expressions into their fast, pre-classified form.
///
/// A loaded [`Mapping`] carries its templates as raw [`TemplateSource`] strings. Before the hot
/// expansion loop runs, every one of them (the identity name, the scope, and each attribute's
/// source, recursively through nested mappings, language maps, properties, and synthetic entities)
/// is compiled once and cached back onto the mapping so per-record expansion never re-parses a
/// template. The top-level source keys those templates read are recorded on the mapping at the same
/// time (see [`SourceReads`]); a synthetic entity's are recorded on its own mapping only.
///
/// Compilation must finish before a [`TemplateResolver`](cassiopeia_mapping::template::resolver::TemplateResolver)
/// is taken from the runner: a resolver only sees the templates registered before it was handed out.
pub struct ExpanderCompiler;

impl ExpanderCompiler {
    /// Compiles every template in `mapping` in place. `origin` names the mapping document in error
    /// messages.
    ///
    /// # Errors
    /// Returns [`MappingError::UncompilableTemplate`] for the first template that cannot be
    /// compiled: no record could ever render it, so the mapping is rejected before any is read.
    pub fn compile(mapping: &mut Mapping, origin: &Path, runner: &mut TemplateRunner) -> Result<()> {
        Self::compile_mapping(mapping, &Arc::from(origin), runner)
    }

    /// Compiles every template in `mapping`, a synthetic entity's included, against the one shared
    /// `document` path.
    fn compile_mapping(mapping: &mut Mapping, document: &Arc<Path>, runner: &mut TemplateRunner) -> Result<()> {
        let mut reads = SourceReads::nothing();
        let compiled_name = Self::compile_template(mapping.identity().entity_name(), document, runner, TemplateSite::EntityName, &mut reads)?;
        mapping.identity_mut().set_compiled_entity_name(Some(compiled_name));

        if let Some(scope) = mapping.scope() {
            let compiled_scope = match scope {
                Scope::Single(source) => CompiledScope::Single(Self::compile_scope(source, document, runner, &mut reads)?),
                Scope::Multiple(sources) => CompiledScope::Multiple(
                    sources
                        .iter()
                        .map(|source| Self::compile_scope(source, document, runner, &mut reads))
                        .collect::<Result<_>>()?,
                ),
            };
            mapping.set_compiled_scope(Some(compiled_scope));
        }

        if let Some(source) = mapping.expires_at() {
            let compiled = Self::compile_template(source, document, runner, TemplateSite::ExpiresAt, &mut reads)?;
            mapping.set_compiled_expires_at(Some(compiled));
        }

        for (name, attribute) in mapping.attributes_mut() {
            Self::compile_attribute(name, None, attribute, document, runner, &mut reads)?;
        }
        mapping.set_source_reads(reads);

        Ok(())
    }

    /// Compiles one `scope` template, rejecting literal text that can never be a written entity's
    /// scope.
    ///
    /// A literal resolves to the same text for every record, so a literal that is no legal scope,
    /// or is the NGSI-LD Null that clause 4.18 reserves for a deleted scope, would skip every record
    /// the mapping reads. An empty literal is kept: it contributes no scope.
    fn compile_scope(source: &TemplateSource, document: &Arc<Path>, runner: &mut TemplateRunner, reads: &mut SourceReads) -> Result<CompiledScopeTemplate> {
        let compiled = Self::compile_template(source, document, runner, TemplateSite::Scope, reads)?;
        match &compiled {
            CompiledTemplate::Static(literal) if !literal.is_empty() => {
                ScopeBuf::for_written_entity(literal.as_str()).map_err(|source| MappingError::UnwritableScope {
                    location: TemplateLocation::new(Arc::clone(document), TemplateSite::Scope),
                    source,
                })?;
            }
            CompiledTemplate::Static(_)
            | CompiledTemplate::Simple(_)
            | CompiledTemplate::Composite(_)
            | CompiledTemplate::Expression(_)
            | CompiledTemplate::Complex(_) => {}
        }

        Ok(CompiledScopeTemplate {
            // The mapping keeps its raw scope declaration, so the compiled scope owns a copy of the
            // template text for a refused record to name.
            source: source.clone(),
            compiled,
        })
    }

    /// Compiles one attribute's source and recurses into everything nested beneath it.
    ///
    /// A failure is reported under the path of `name`, the key the attribute is declared under,
    /// held beneath the declarations `owners` frames; a per-language entry or an instance has no key
    /// of its own and is reported under its parent's.
    fn compile_attribute(
        name: &NameBuf,
        owners: Option<&DeclarationTrail<'_>>,
        attribute: &mut Attribute,
        document: &Arc<Path>,
        runner: &mut TemplateRunner,
        reads: &mut SourceReads,
    ) -> Result<()> {
        let path = DeclarationTrail::path_of(owners, name);
        let compiled = Self::compile_source_templates(attribute.source().as_ref(), &path, document, runner, reads)?;
        attribute.set_compiled_source(compiled);

        let members = DeclarationTrail::below(name, Nesting::Member, owners);
        for (nested_name, nested) in attribute.mappings_mut() {
            Self::compile_attribute(nested_name, Some(&members), nested, document, runner, reads)?;
        }
        for language in attribute.language_map_mut().values_mut() {
            Self::compile_attribute(name, owners, language, document, runner, reads)?;
        }
        let sub_attributes = DeclarationTrail::below(name, Nesting::SubAttribute, owners);
        if let Some(properties) = attribute.properties_mut() {
            for (property_name, property) in properties {
                Self::compile_attribute(property_name, Some(&sub_attributes), property, document, runner, reads)?;
            }
        }
        if let Some(instances) = attribute.instances_mut() {
            for instance in instances {
                Self::compile_instance(&path, &sub_attributes, instance, document, runner, reads)?;
            }
        }
        if let Some(synthetic) = attribute.synthetic_entity_mut() {
            Self::compile_mapping(synthetic, document, runner)?;
        }

        Ok(())
    }

    /// Compiles one multi-instance instance's source and its own properties, reporting a failure in
    /// its source under `path`, the attribute declaring the instance, and one in a property beneath
    /// `sub_attributes`, that attribute's frame.
    fn compile_instance(
        path: &AttributePath,
        sub_attributes: &DeclarationTrail<'_>,
        instance: &mut AttributeInstance,
        document: &Arc<Path>,
        runner: &mut TemplateRunner,
        reads: &mut SourceReads,
    ) -> Result<()> {
        let compiled = Self::compile_source_templates(instance.source().as_ref(), path, document, runner, reads)?;
        instance.set_compiled_source(compiled);

        if let Some(properties) = instance.properties_mut() {
            for (property_name, property) in properties {
                Self::compile_attribute(property_name, Some(sub_attributes), property, document, runner, reads)?;
            }
        }

        Ok(())
    }

    /// Compiles a `source` value into its templates: a single string, a list of them, or a literal
    /// carried verbatim. Returns `None` when there is no source to compile.
    fn compile_source_templates(
        source: Option<&Value>,
        path: &AttributePath,
        document: &Arc<Path>,
        runner: &mut TemplateRunner,
        reads: &mut SourceReads,
    ) -> Result<Option<Vec<CompiledTemplate>>> {
        let mut compiled = Vec::new();
        // Each compiled template keeps the site it is declared at, so the attribute path is copied
        // once per template.
        let site = || TemplateSite::Attribute(path.clone());

        if let Some(source) = source {
            match source {
                Value::String(text) => compiled.push(Self::compile_template(&TemplateSource::new(text), document, runner, site(), reads)?),
                Value::Array(items) => {
                    for item in items {
                        match item {
                            Value::String(text) => compiled.push(Self::compile_template(&TemplateSource::new(text), document, runner, site(), reads)?),
                            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
                                compiled.push(CompiledTemplate::Static(item.to_string()));
                            }
                        }
                    }
                }
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {
                    compiled.push(CompiledTemplate::Static(source.to_string()));
                }
            }
        }

        Ok(if compiled.is_empty() { None } else { Some(compiled) })
    }

    /// Compiles one template declared at `site` in the mapping document at `document`, attributing a
    /// failure, at load or on any record, to that declaration, and adds the source keys it reads to
    /// `reads`.
    fn compile_template(
        source: &TemplateSource,
        document: &Arc<Path>,
        runner: &mut TemplateRunner,
        site: TemplateSite,
        reads: &mut SourceReads,
    ) -> Result<CompiledTemplate> {
        let location = TemplateLocation::new(Arc::clone(document), site);

        let compiled = runner
            .compile(source, &location)
            .map_err(|source| MappingError::UncompilableTemplate { location, source })?;
        reads.absorb(runner.source_reads(&compiled));
        Ok(compiled)
    }
}

#[cfg(test)]
mod tests {
    use crate::compiler::ExpanderCompiler;
    use cassiopeia_mapping::{
        error::MappingError,
        mapping::Mapping,
        source_reads::{SourceKey, SourceReads},
        template::{CompiledTemplate, TemplateSource, runner::TemplateRunner},
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, error::NgsiLdError, name::NameBuf};
    use pretty_assertions::assert_eq;
    use std::{error::Error, path::Path};

    const ORIGIN: &str = "sensor.json5";

    /// A mapping document declaring `attributes`, with `identity` and `scope` members spliced in.
    fn document(identity: &str, scope: &str, attributes: &str) -> String {
        format!(r#"{{ version: "v4", dataModel: "Sensor", identity: {{ entityName: "{identity}" }}, {scope} attributes: {attributes} }}"#)
    }

    fn compile(document: &str) -> Result<Mapping, MappingError> {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new(ORIGIN), &mut runner).unwrap();

        ExpanderCompiler::compile(&mut mapping, Path::new(ORIGIN), &mut runner).map(|()| mapping)
    }

    /// The site and template of a rejection, after checking it names the document.
    fn rejection(document: &str) -> (TemplateSite, TemplateSource) {
        match compile(document) {
            Err(MappingError::UncompilableTemplate { location, source }) => {
                assert_eq!(*location.document, *Path::new(ORIGIN));
                (location.site, source.template().clone())
            }
            other => panic!("expected an uncompilable template, got {other:?}"),
        }
    }

    /// The hint a rejection chains under its headline.
    fn hint(document: &str) -> String {
        let error = compile(document).unwrap_err();

        error.source().expect("the hint is chained").to_string()
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    fn attribute(value: &str) -> TemplateSite {
        TemplateSite::Attribute(AttributePath::top_level(name(value)))
    }

    /// The site of the sub-attribute `sub_attributes` name, outermost first, beneath `root`.
    fn sub_attribute(root: &str, sub_attributes: &[&str]) -> TemplateSite {
        TemplateSite::Attribute(AttributePath::new(
            name(root),
            sub_attributes.iter().map(|value| name(value)).collect(),
            Vec::new(),
        ))
    }

    #[test]
    fn a_mapping_whose_templates_all_parse_compiles_every_one_in_place() {
        let mapping = compile(&document(
            "S-{{ id }}",
            r#"scope: "/{{ city }}","#,
            r#"{ temperature: { source: "{{ t | upper }}" } }"#,
        ))
        .unwrap();

        assert!(mapping.identity().compiled_entity_name().is_some());
        assert!(mapping.compiled_scope().is_some());
        let temperature = &mapping.attributes()[&NameBuf::new("temperature").unwrap()];
        assert!(matches!(temperature.compiled_source().as_deref(), Some([CompiledTemplate::Expression(_)])));
    }

    #[test]
    fn an_attribute_source_with_a_syntax_error_is_rejected_naming_the_document_and_the_attribute() {
        let document = document("S-{{ id }}", "", r#"{ temperature: { source: "{{ t | upper " } }"#);

        assert_eq!(rejection(&document), (attribute("temperature"), TemplateSource::new("{{ t | upper ")));
        let message = compile(&document).unwrap_err().to_string();
        assert!(message.contains(ORIGIN));
        assert!(message.contains("temperature"));
    }

    #[test]
    fn an_attribute_source_with_an_unclosed_conditional_is_rejected() {
        let document = document("S-{{ id }}", "", r#"{ temperature: { source: "{% if t %}{{ t | upper }}" } }"#);

        assert_eq!(
            rejection(&document),
            (attribute("temperature"), TemplateSource::new("{% if t %}{{ t | upper }}"))
        );
    }

    #[test]
    fn an_attribute_source_with_an_unclosed_field_reference_is_rejected_naming_the_document_and_the_attribute() {
        let document = document("S-{{ id }}", "", r#"{ stationName: { source: "Station-{{ id" } }"#);

        assert_eq!(rejection(&document), (attribute("stationName"), TemplateSource::new("Station-{{ id")));
        let message = compile(&document).unwrap_err().to_string();
        assert!(message.contains(ORIGIN));
        assert!(message.contains("stationName"));
    }

    #[test]
    fn an_entity_name_with_an_unclosed_field_reference_is_rejected() {
        let document = document("S-{{ id", "", "{}");

        assert_eq!(rejection(&document), (TemplateSite::EntityName, TemplateSource::new("S-{{ id")));
    }

    #[test]
    fn an_attribute_source_with_an_unfiltered_unclosed_conditional_is_rejected() {
        let document = document("S-{{ id }}", "", r#"{ temperature: { source: "{%if t%}{{ t }}" } }"#);

        assert_eq!(rejection(&document), (attribute("temperature"), TemplateSource::new("{%if t%}{{ t }}")));
    }

    #[test]
    fn an_attribute_source_with_an_unregistered_filter_is_rejected() {
        let document = document("S-{{ id }}", "", r#"{ temperature: { source: "{{ t | no_such_filter }}" } }"#);

        assert_eq!(rejection(&document).0, attribute("temperature"));
    }

    #[test]
    fn a_list_source_entry_with_a_syntax_error_is_rejected() {
        let document = document("S-{{ id }}", "", r#"{ refs: { source: ["{{ a }}", "{{ b | upper "] } }"#);

        assert_eq!(rejection(&document), (attribute("refs"), TemplateSource::new("{{ b | upper ")));
    }

    #[test]
    fn an_entity_name_with_a_syntax_error_is_rejected() {
        let document = document("S-{{ id | upper ", "", "{}");

        assert_eq!(rejection(&document), (TemplateSite::EntityName, TemplateSource::new("S-{{ id | upper ")));
    }

    #[test]
    fn a_scope_with_a_syntax_error_is_rejected() {
        let document = document("S-{{ id }}", r#"scope: "/{{ city | lower ","#, "{}");

        assert_eq!(rejection(&document), (TemplateSite::Scope, TemplateSource::new("/{{ city | lower ")));
    }

    /// The reason a mapping with an unwritable literal scope is refused, after checking the refusal
    /// names the document and the `scope` member.
    fn unwritable_scope(document: &str) -> NgsiLdError {
        match compile(document) {
            Err(MappingError::UnwritableScope { location, source }) => {
                assert_eq!(*location.document, *Path::new(ORIGIN));
                assert_eq!(location.site, TemplateSite::Scope);
                source
            }
            other => panic!("expected an unwritable scope, got {other:?}"),
        }
    }

    #[test]
    fn a_literal_ngsi_ld_null_scope_is_rejected_naming_the_document_and_the_scope() {
        let document = document("S-{{ id }}", r#"scope: "urn:ngsi-ld:null","#, "{}");

        assert!(matches!(unwritable_scope(&document), NgsiLdError::NullScope));
        assert!(compile(&document).unwrap_err().to_string().contains(ORIGIN));
        assert_eq!(hint(&document), NgsiLdError::NullScope.to_string());
    }

    #[test]
    fn a_literal_ngsi_ld_null_in_a_scope_list_is_rejected() {
        let document = document("S-{{ id }}", r#"scope: ["/Madrid", "urn:ngsi-ld:null"],"#, "{}");

        assert!(matches!(unwritable_scope(&document), NgsiLdError::NullScope));
    }

    #[test]
    fn a_literal_malformed_scope_is_rejected_as_illegal() {
        let document = document("S-{{ id }}", r#"scope: "/Madrid Centro","#, "{}");

        assert!(matches!(unwritable_scope(&document), NgsiLdError::InvalidScope { rejected } if &*rejected == "/Madrid Centro"));
    }

    #[test]
    fn a_synthetic_entity_s_literal_ngsi_ld_null_scope_is_rejected() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ owner: { type: "Relationship", source: "{{ id }}", target: { entity: "Organization" }, syntheticEntity: { dataModel: "Organization", identity: { entityName: "O-{{ id }}" }, scope: "urn:ngsi-ld:null", attributes: {} } } }"#,
        );

        assert!(matches!(unwritable_scope(&document), NgsiLdError::NullScope));
    }

    #[test]
    fn a_literal_legal_scope_and_an_empty_literal_scope_compile() {
        for scope in [r#"scope: "/Madrid/Gardens","#, r#"scope: "","#] {
            assert!(compile(&document("S-{{ id }}", scope, "{}")).is_ok(), "{scope}");
        }
    }

    #[test]
    fn a_scope_template_that_may_resolve_to_the_ngsi_ld_null_compiles() {
        assert!(compile(&document("S-{{ id }}", r#"scope: "{{ s }}","#, "{}")).is_ok());
    }

    #[test]
    fn an_expires_at_with_a_syntax_error_is_rejected_naming_the_site() {
        let document = document("S-{{ id }}", r#"expiresAt: "{{ until | lower ","#, "{}");

        assert_eq!(rejection(&document), (TemplateSite::ExpiresAt, TemplateSource::new("{{ until | lower ")));
        assert!(compile(&document).unwrap_err().to_string().contains("expiresAt"));
    }

    #[test]
    fn the_expires_at_source_is_read() {
        let mapping = compile(&document(
            "{{ id }}",
            r#"expiresAt: "{{ until }}","#,
            r#"{ t: { source: "{{ temperature }}" } }"#,
        ))
        .unwrap();

        assert_eq!(mapping.source_reads(), &keys(&["id", "temperature", "until"]));
        assert!(mapping.compiled_expires_at().is_some());
    }

    #[test]
    fn a_scope_list_entry_with_a_syntax_error_is_rejected() {
        let document = document("S-{{ id }}", r#"scope: ["/Ljubljana", "/{% if zone %}{{ zone | lower }}"],"#, "{}");

        assert_eq!(rejection(&document).0, TemplateSite::Scope);
    }

    #[test]
    fn a_nested_mapping_with_a_syntax_error_is_reported_as_a_member_of_its_attribute() {
        let document = document("S-{{ id }}", "", r#"{ address: { mappings: { city: { source: "{{ city | upper " } } } }"#);

        assert_eq!(
            rejection(&document).0,
            TemplateSite::Attribute(AttributePath::new(name("address"), Vec::new(), vec![name("city")]))
        );
    }

    #[test]
    fn a_property_with_a_syntax_error_is_reported_under_its_path_from_the_attribute() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ temperature: { source: "{{ t }}", properties: { unitCode: { source: "{{ u | upper " } } } }"#,
        );

        assert_eq!(rejection(&document).0, sub_attribute("temperature", &["unitCode"]));
        assert!(compile(&document).unwrap_err().to_string().contains("attribute `temperature.unitCode`"));
    }

    #[test]
    fn a_property_of_a_relationship_with_a_syntax_error_is_reported_under_its_path_from_the_relationship() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ hasLeadActor: { type: "Relationship", source: "{{ actor }}", target: { entity: "Person" }, properties: { castSize: { source: "{{ n | lower " } } } }"#,
        );

        assert_eq!(rejection(&document).0, sub_attribute("hasLeadActor", &["castSize"]));
    }

    #[test]
    fn a_sub_attribute_two_levels_down_with_a_syntax_error_is_reported_under_its_whole_path() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ temperature: { source: "{{ t }}", properties: { accuracy: { source: "{{ a }}", properties: { level: { source: "{{ l | upper " } } } } } }"#,
        );

        assert_eq!(rejection(&document).0, sub_attribute("temperature", &["accuracy", "level"]));
    }

    #[test]
    fn a_compiled_observed_at_qualifier_remembers_its_path_from_the_attribute() {
        let mapping = compile(&document(
            "S-{{ id }}",
            "",
            r#"{ temperature: { source: "{{ t }}", properties: { observedAt: { source: "{{ at | upper }}" } } } }"#,
        ))
        .unwrap();

        let temperature = &mapping.attributes()[&name("temperature")];
        let observed_at = &temperature.properties().as_ref().expect("the attribute declares properties")[&name("observedAt")];
        let Some([CompiledTemplate::Expression(template)]) = observed_at.compiled_source().as_deref() else {
            panic!("expected one typed Tera template");
        };
        assert_eq!(template.location.site, sub_attribute("temperature", &["observedAt"]));
    }

    #[test]
    fn a_property_of_an_instance_with_a_syntax_error_is_reported_under_the_attribute_declaring_the_instance() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ temperature: { instances: [{ source: "{{ a }}", properties: { unitCode: { source: "{{ u | upper " } } }] } }"#,
        );

        assert_eq!(rejection(&document).0, sub_attribute("temperature", &["unitCode"]));
    }

    #[test]
    fn a_language_map_entry_with_a_syntax_error_is_reported_under_its_attribute() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ name: { type: "LanguageProperty", languageMap: { en: { source: "{{ en | upper " } } } }"#,
        );

        assert_eq!(rejection(&document).0, attribute("name"));
    }

    #[test]
    fn an_instance_source_with_a_syntax_error_is_reported_under_its_attribute() {
        let document = document("S-{{ id }}", "", r#"{ temperature: { instances: [{ source: "{{ a | float " }] } }"#);

        assert_eq!(rejection(&document), (attribute("temperature"), TemplateSource::new("{{ a | float ")));
    }

    #[test]
    fn a_synthetic_entity_template_with_a_syntax_error_is_rejected() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ owner: { type: "Relationship", source: "{{ id }}", target: { entity: "Organization" }, syntheticEntity: { dataModel: "Organization", identity: { entityName: "O-{{ id | upper " }, attributes: {} } } }"#,
        );

        assert_eq!(rejection(&document), (TemplateSite::EntityName, TemplateSource::new("O-{{ id | upper ")));
    }

    #[test]
    fn an_attribute_source_with_a_hyphenated_name_is_rejected_naming_the_attribute_and_the_token() {
        let document = document("S-{{ id }}", "", r#"{ stationId: { source: "{{ station-id }}" } }"#);

        assert_eq!(rejection(&document), (attribute("stationId"), TemplateSource::new("{{ station-id }}")));
        assert!(compile(&document).unwrap_err().to_string().contains("stationId"));
        assert_eq!(
            hint(&document),
            "`station-id` in `{{ station-id }}` is ambiguous: write `this['station-id']` to read the field, or `station - id` to subtract"
        );
    }

    #[test]
    fn an_attribute_source_with_a_non_ascii_name_in_an_expression_is_rejected_suggesting_the_bracketed_spelling() {
        let document = document("S-{{ id }}", "", r#"{ time: { source: "{{ čas | upper }}" } }"#);

        assert_eq!(rejection(&document), (attribute("time"), TemplateSource::new("{{ čas | upper }}")));
        assert_eq!(
            hint(&document),
            "`čas` in `{{ čas | upper }}` is not a name Tera can read: write `this['čas']` to read the field"
        );
    }

    #[test]
    fn an_attribute_source_with_a_lone_non_ascii_name_compiles_to_a_direct_lookup() {
        let mapping = compile(&document("S-{{ id }}", "", r#"{ time: { source: "{{ čas }}" } }"#)).unwrap();

        let time = &mapping.attributes()[&NameBuf::new("time").unwrap()];
        assert!(matches!(time.compiled_source().as_deref(), Some([CompiledTemplate::Simple(key)]) if key.as_str() == "čas"));
    }

    #[test]
    fn an_entity_name_with_a_hyphenated_name_is_rejected_naming_the_identity() {
        let document = document("{{ station-id }}", "", "{}");

        assert_eq!(rejection(&document), (TemplateSite::EntityName, TemplateSource::new("{{ station-id }}")));
        assert!(compile(&document).unwrap_err().to_string().contains("identity.entityName"));
    }

    #[test]
    fn a_compiled_tera_template_remembers_the_document_and_the_attribute_declaring_it() {
        let mapping = compile(&document("S-{{ id }}", "", r#"{ temperature: { source: "{{ t | upper }}" } }"#)).unwrap();

        let temperature = &mapping.attributes()[&NameBuf::new("temperature").unwrap()];
        let Some([CompiledTemplate::Expression(template)]) = temperature.compiled_source().as_deref() else {
            panic!("expected one typed Tera template");
        };
        assert_eq!(*template.location.document, *Path::new(ORIGIN));
        assert_eq!(template.location.site, attribute("temperature"));
    }

    fn keys(names: &[&str]) -> SourceReads {
        SourceReads::Keys(names.iter().map(|name| SourceKey::new(name)).collect())
    }

    #[test]
    fn a_mapping_not_yet_compiled_is_taken_to_read_everything() {
        let mut runner = TemplateRunner::new();
        let mapping = Mapping::from_json5(&document("{{ id }}", "", "{}"), Path::new(ORIGIN), &mut runner).unwrap();

        assert_eq!(mapping.source_reads(), &SourceReads::Everything);
    }

    #[test]
    fn the_identity_scope_and_attribute_sources_are_all_read() {
        let mapping = compile(&document(
            "{{ id }}",
            r#"scope: "/{{ region }}","#,
            r#"{ t: { source: "{{ temperature }}" }, location: { type: "GeoProperty", source: ["{{ lon }}", "{{ lat }}"], transformation: "point" } }"#,
        ))
        .unwrap();

        assert_eq!(mapping.source_reads(), &keys(&["id", "lat", "lon", "region", "temperature"]));
    }

    #[test]
    fn properties_observed_at_and_nested_mappings_are_read() {
        let mapping = compile(&document(
            "{{ id }}",
            "",
            r#"{
                t: { source: "{{ temperature }}", properties: { observedAt: { source: "{{ ts }}" }, unitCode: { source: "CEL" } } },
                address: { type: "Property", mappings: { city: { source: "{{ town }}" } } },
            }"#,
        ))
        .unwrap();

        assert_eq!(mapping.source_reads(), &keys(&["id", "temperature", "town", "ts"]));
    }

    #[test]
    fn relationship_sources_and_every_instance_source_are_read() {
        let mapping = compile(&document(
            "{{ id }}",
            "",
            r#"{
                owner: { type: "Relationship", source: "{{ owner_id }}", target: { entity: "Organization" } },
                servesAirport: {
                    type: "Relationship",
                    target: { entity: "Airport" },
                    instances: [
                        { source: "{{ departure }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:{{ leg }}" } } },
                        { source: "{{ arrival }}" },
                    ],
                },
            }"#,
        ))
        .unwrap();

        assert_eq!(mapping.source_reads(), &keys(&["arrival", "departure", "id", "leg", "owner_id"]));
    }

    #[test]
    fn a_synthetic_entity_s_reads_are_its_own_and_not_its_parent_s() {
        let mapping = compile(&document(
            "{{ id }}",
            "",
            r#"{
                owner: {
                    type: "Relationship",
                    source: "{{ owner_id }}",
                    target: { entity: "Organization" },
                    syntheticEntity: { dataModel: "Organization", identity: { entityName: "{{ owner_id }}" }, attributes: { name: { source: "{{ owner_name }}" } } },
                },
            }"#,
        ))
        .unwrap();
        let synthetic = mapping
            .attributes()
            .values()
            .find_map(|attribute| attribute.synthetic_entity().as_ref())
            .unwrap();

        assert_eq!(mapping.source_reads(), &keys(&["id", "owner_id"]));
        assert_eq!(synthetic.source_reads(), &keys(&["owner_id", "owner_name"]));
    }

    #[test]
    fn one_template_reading_the_whole_record_makes_the_mapping_read_everything() {
        let mapping = compile(&document("{{ id }}", "", r#"{ raw: { type: "JsonProperty", source: "{{ context }}" } }"#)).unwrap();

        assert_eq!(mapping.source_reads(), &SourceReads::Everything);
    }
}

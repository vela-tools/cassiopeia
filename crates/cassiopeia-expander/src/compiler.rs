use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    error::{MappingError, Result},
    mapping::Mapping,
    scope::{CompiledScope, Scope},
    template::{CompiledTemplate, TemplateSource, runner::TemplateRunner},
    template_location::TemplateLocation,
    template_site::TemplateSite,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use serde_json::Value;
use std::{path::Path, sync::Arc};

/// Compiles a mapping's raw template expressions into their fast, pre-classified form.
///
/// A loaded [`Mapping`] carries its templates as raw [`TemplateSource`] strings. Before the hot
/// expansion loop runs, every one of them (the identity name, the scope, and each attribute's
/// source, recursively through nested mappings, language maps, properties, and synthetic entities)
/// is compiled once and cached back onto the mapping so per-record expansion never re-parses a
/// template.
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
        let compiled_name = Self::compile_template(mapping.identity().entity_name(), document, runner, TemplateSite::EntityName)?;
        mapping.identity_mut().set_compiled_entity_name(Some(compiled_name));

        if let Some(scope) = mapping.scope() {
            let compiled_scope = match scope {
                Scope::Single(source) => CompiledScope::Single(Self::compile_template(source, document, runner, TemplateSite::Scope)?),
                Scope::Multiple(sources) => CompiledScope::Multiple(
                    sources
                        .iter()
                        .map(|source| Self::compile_template(source, document, runner, TemplateSite::Scope))
                        .collect::<Result<_>>()?,
                ),
            };
            mapping.set_compiled_scope(Some(compiled_scope));
        }

        for (name, attribute) in mapping.attributes_mut() {
            Self::compile_attribute(name, attribute, document, runner)?;
        }

        Ok(())
    }

    /// Compiles one attribute's source and recurses into everything nested beneath it.
    ///
    /// A failure is reported under `name`, the key the attribute is declared under; a per-language
    /// entry or an instance has no key of its own and is reported under its parent's.
    fn compile_attribute(name: &NameBuf, attribute: &mut Attribute, document: &Arc<Path>, runner: &mut TemplateRunner) -> Result<()> {
        let compiled = Self::compile_source_templates(attribute.source().as_ref(), name, document, runner)?;
        attribute.set_compiled_source(compiled);

        for (nested_name, nested) in attribute.mappings_mut() {
            Self::compile_attribute(nested_name, nested, document, runner)?;
        }
        for language in attribute.language_map_mut().values_mut() {
            Self::compile_attribute(name, language, document, runner)?;
        }
        if let Some(properties) = attribute.properties_mut() {
            for (property_name, property) in properties {
                Self::compile_attribute(property_name, property, document, runner)?;
            }
        }
        if let Some(instances) = attribute.instances_mut() {
            for instance in instances {
                Self::compile_instance(name, instance, document, runner)?;
            }
        }
        if let Some(synthetic) = attribute.synthetic_entity_mut() {
            Self::compile_mapping(synthetic, document, runner)?;
        }

        Ok(())
    }

    /// Compiles one multi-instance instance's source and its own properties, reporting a failure in
    /// its source under `name`, the attribute declaring the instance.
    fn compile_instance(name: &NameBuf, instance: &mut AttributeInstance, document: &Arc<Path>, runner: &mut TemplateRunner) -> Result<()> {
        let compiled = Self::compile_source_templates(instance.source().as_ref(), name, document, runner)?;
        instance.set_compiled_source(compiled);

        if let Some(properties) = instance.properties_mut() {
            for (property_name, property) in properties {
                Self::compile_attribute(property_name, property, document, runner)?;
            }
        }

        Ok(())
    }

    /// Compiles a `source` value into its templates: a single string, a list of them, or a literal
    /// carried verbatim. Returns `None` when there is no source to compile.
    fn compile_source_templates(
        source: Option<&Value>,
        name: &NameBuf,
        document: &Arc<Path>,
        runner: &mut TemplateRunner,
    ) -> Result<Option<Vec<CompiledTemplate>>> {
        let mut compiled = Vec::new();
        // Each compiled template keeps the site it is declared at, so the attribute name is copied
        // once per template.
        let site = || TemplateSite::Attribute(name.clone());

        if let Some(source) = source {
            match source {
                Value::String(text) => compiled.push(Self::compile_template(&TemplateSource::new(text), document, runner, site())?),
                Value::Array(items) => {
                    for item in items {
                        match item {
                            Value::String(text) => compiled.push(Self::compile_template(&TemplateSource::new(text), document, runner, site())?),
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
    /// failure, at load or on any record, to that declaration.
    fn compile_template(source: &TemplateSource, document: &Arc<Path>, runner: &mut TemplateRunner, site: TemplateSite) -> Result<CompiledTemplate> {
        let location = TemplateLocation::new(Arc::clone(document), site);

        runner
            .compile(source, &location)
            .map_err(|source| MappingError::UncompilableTemplate { location, source })
    }
}

#[cfg(test)]
mod tests {
    use crate::compiler::ExpanderCompiler;
    use cassiopeia_mapping::{
        error::MappingError,
        mapping::Mapping,
        template::{CompiledTemplate, TemplateSource, runner::TemplateRunner},
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
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

    fn attribute(name: &str) -> TemplateSite {
        TemplateSite::Attribute(NameBuf::new(name).unwrap())
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

    #[test]
    fn a_scope_list_entry_with_a_syntax_error_is_rejected() {
        let document = document("S-{{ id }}", r#"scope: ["/Ljubljana", "/{% if zone %}{{ zone | lower }}"],"#, "{}");

        assert_eq!(rejection(&document).0, TemplateSite::Scope);
    }

    #[test]
    fn a_nested_mapping_with_a_syntax_error_is_reported_under_its_own_key() {
        let document = document("S-{{ id }}", "", r#"{ address: { mappings: { city: { source: "{{ city | upper " } } } }"#);

        assert_eq!(rejection(&document).0, attribute("city"));
    }

    #[test]
    fn a_property_with_a_syntax_error_is_reported_under_the_property() {
        let document = document(
            "S-{{ id }}",
            "",
            r#"{ temperature: { source: "{{ t }}", properties: { unitCode: { source: "{{ u | upper " } } } }"#,
        );

        assert_eq!(rejection(&document).0, attribute("unitCode"));
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
}

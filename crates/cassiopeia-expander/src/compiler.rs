use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    error::{MappingError, Result},
    mapping::Mapping,
    scope::{CompiledScope, Scope},
    template::{CompiledTemplate, TemplateSource, runner::TemplateRunner},
    template_site::TemplateSite,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use serde_json::Value;
use std::path::Path;

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
        let compiled_name = Self::compile_template(mapping.identity().entity_name(), origin, runner, || TemplateSite::EntityName)?;
        mapping.identity_mut().set_compiled_entity_name(Some(compiled_name));

        if let Some(scope) = mapping.scope() {
            let compiled_scope = match scope {
                Scope::Single(source) => CompiledScope::Single(Self::compile_template(source, origin, runner, || TemplateSite::Scope)?),
                Scope::Multiple(sources) => CompiledScope::Multiple(
                    sources
                        .iter()
                        .map(|source| Self::compile_template(source, origin, runner, || TemplateSite::Scope))
                        .collect::<Result<_>>()?,
                ),
            };
            mapping.set_compiled_scope(Some(compiled_scope));
        }

        for (name, attribute) in mapping.attributes_mut() {
            Self::compile_attribute(name, attribute, origin, runner)?;
        }

        Ok(())
    }

    /// Compiles one attribute's source and recurses into everything nested beneath it.
    ///
    /// A failure is reported under `name`, the key the attribute is declared under; a per-language
    /// entry or an instance has no key of its own and is reported under its parent's.
    fn compile_attribute(name: &NameBuf, attribute: &mut Attribute, origin: &Path, runner: &mut TemplateRunner) -> Result<()> {
        let compiled = Self::compile_source_templates(attribute.source().as_ref(), name, origin, runner)?;
        attribute.set_compiled_source(compiled);

        for (nested_name, nested) in attribute.mappings_mut() {
            Self::compile_attribute(nested_name, nested, origin, runner)?;
        }
        for language in attribute.language_map_mut().values_mut() {
            Self::compile_attribute(name, language, origin, runner)?;
        }
        if let Some(properties) = attribute.properties_mut() {
            for (property_name, property) in properties {
                Self::compile_attribute(property_name, property, origin, runner)?;
            }
        }
        if let Some(instances) = attribute.instances_mut() {
            for instance in instances {
                Self::compile_instance(name, instance, origin, runner)?;
            }
        }
        if let Some(synthetic) = attribute.synthetic_entity_mut() {
            Self::compile(synthetic, origin, runner)?;
        }

        Ok(())
    }

    /// Compiles one multi-instance instance's source and its own properties, reporting a failure in
    /// its source under `name`, the attribute declaring the instance.
    fn compile_instance(name: &NameBuf, instance: &mut AttributeInstance, origin: &Path, runner: &mut TemplateRunner) -> Result<()> {
        let compiled = Self::compile_source_templates(instance.source().as_ref(), name, origin, runner)?;
        instance.set_compiled_source(compiled);

        if let Some(properties) = instance.properties_mut() {
            for (property_name, property) in properties {
                Self::compile_attribute(property_name, property, origin, runner)?;
            }
        }

        Ok(())
    }

    /// Compiles a `source` value into its templates: a single string, a list of them, or a literal
    /// carried verbatim. Returns `None` when there is no source to compile.
    fn compile_source_templates(source: Option<&Value>, name: &NameBuf, origin: &Path, runner: &mut TemplateRunner) -> Result<Option<Vec<CompiledTemplate>>> {
        let mut compiled = Vec::new();
        // The site is built only once a template has failed, so the name is copied on the error path
        // alone.
        let site = || TemplateSite::Attribute(name.clone());

        if let Some(source) = source {
            match source {
                Value::String(text) => compiled.push(Self::compile_template(&TemplateSource::new(text), origin, runner, site)?),
                Value::Array(items) => {
                    for item in items {
                        match item {
                            Value::String(text) => compiled.push(Self::compile_template(&TemplateSource::new(text), origin, runner, site)?),
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

    /// Compiles one template, attributing a failure to the declaration `site` names in the document
    /// at `origin`.
    fn compile_template(source: &TemplateSource, origin: &Path, runner: &mut TemplateRunner, site: impl FnOnce() -> TemplateSite) -> Result<CompiledTemplate> {
        runner.compile(source).map_err(|source| MappingError::UncompilableTemplate {
            path: origin.to_path_buf(),
            site: site(),
            source: Box::new(source),
        })
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
    use std::path::Path;

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
            Err(MappingError::UncompilableTemplate { path, site, source }) => {
                assert_eq!(path, Path::new(ORIGIN));
                (site, source.template)
            }
            other => panic!("expected an uncompilable template, got {other:?}"),
        }
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
}

use cassiopeia_mapping::{
    attribute::{Attribute, Attributes},
    identity::Identity,
    template::CompiledTemplate,
};

/// Structural checks over a mapping's identity and attributes, used to choose a URN deduplication
/// strategy.
///
/// A template the runner compiled to anything but a literal is "dynamic": it can produce a different
/// value per record. A literal is "static": it produces the same value for every record. Reading the
/// compiled form rather than the source text keeps this the runner's own classification, so a
/// template with only a `{% %}` tag is as dynamic here as it is to the resolver.
pub(crate) struct Analysis;

impl Analysis {
    /// Whether the identity's entity-name template is static. An identity not yet compiled is not.
    pub(crate) fn is_identity_static(identity: &Identity) -> bool {
        identity.compiled_entity_name().as_ref().is_some_and(Self::is_literal)
    }

    /// Whether every attribute in the map, recursively, is static.
    pub(crate) fn is_attributes_static(attributes: &Attributes) -> bool {
        attributes.values().all(Self::is_attribute_static)
    }

    /// Whether one attribute and everything nested beneath it is static.
    ///
    /// An attribute with no compiled source reads no record field: it has no source, or a literal
    /// one the URN generator takes verbatim.
    fn is_attribute_static(attribute: &Attribute) -> bool {
        let source_is_static = attribute
            .compiled_source()
            .as_ref()
            .is_none_or(|templates| templates.iter().all(Self::is_literal));

        source_is_static && attribute.mappings().values().all(Self::is_attribute_static)
    }

    /// Whether a compiled template is a literal.
    const fn is_literal(template: &CompiledTemplate) -> bool {
        match template {
            CompiledTemplate::Static(_) => true,
            CompiledTemplate::Simple(_) | CompiledTemplate::Composite(_) | CompiledTemplate::Expression(_) | CompiledTemplate::Complex(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{compiler::ExpanderCompiler, urn::analysis::Analysis};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use std::path::Path;

    /// Loads and compiles a mapping with the given entity name and attributes.
    fn compiled(entity_name: &str, attributes: &str) -> Mapping {
        let document = format!(r#"{{ version: "v4", dataModel: "Sensor", identity: {{ entityName: "{entity_name}" }}, attributes: {attributes} }}"#);
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(&document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, Path::new("test.json5"), &mut runner).unwrap();

        mapping
    }

    #[test]
    fn a_literal_identity_is_static() {
        assert!(Analysis::is_identity_static(compiled("Sensor-1", "{}").identity()));
    }

    #[test]
    fn an_identity_reading_a_field_is_dynamic() {
        assert!(!Analysis::is_identity_static(compiled("Sensor-{{ id }}", "{}").identity()));
    }

    #[test]
    fn an_identity_built_only_from_a_tag_is_dynamic() {
        let mapping = compiled("{% if code %}Coded{% else %}Uncoded{% endif %}", "{}");

        assert!(!Analysis::is_identity_static(mapping.identity()));
    }

    #[test]
    fn literal_attribute_sources_are_static() {
        let mapping = compiled("Sensor-1", r#"{ label: { source: "fixed" }, count: { source: 3 } }"#);

        assert!(Analysis::is_attributes_static(mapping.attributes()));
    }

    #[test]
    fn an_attribute_source_reading_a_field_is_dynamic() {
        let mapping = compiled("Sensor-1", r#"{ label: { source: "fixed" }, reading: { source: "{{ t }}" } }"#);

        assert!(!Analysis::is_attributes_static(mapping.attributes()));
    }

    #[test]
    fn an_attribute_source_built_only_from_a_tag_is_dynamic() {
        let mapping = compiled("Sensor-1", r#"{ label: { source: "{% if a %}on{% else %}off{% endif %}" } }"#);

        assert!(!Analysis::is_attributes_static(mapping.attributes()));
    }

    #[test]
    fn a_nested_attribute_reading_a_field_makes_its_parent_dynamic() {
        let mapping = compiled("Sensor-1", r#"{ address: { mappings: { city: { source: "{{ city }}" } } } }"#);

        assert!(!Analysis::is_attributes_static(mapping.attributes()));
    }
}

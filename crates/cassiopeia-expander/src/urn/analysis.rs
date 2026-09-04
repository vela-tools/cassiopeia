use cassiopeia_mapping::{
    attribute::{Attribute, Attributes},
    identity::Identity,
};

/// Structural checks over a mapping's identity and attributes, used to choose a URN deduplication
/// strategy.
///
/// A template still carrying `{{` after load is "dynamic": it produces a different value per record.
/// One that does not is "static": it produces the same value for every record.
pub(crate) struct Analysis;

impl Analysis {
    /// Whether the identity's entity-name template is static.
    pub(crate) fn is_identity_static(identity: &Identity) -> bool {
        !identity.entity_name().as_str().contains("{{")
    }

    /// Whether every attribute in the map, recursively, is static.
    pub(crate) fn is_attributes_static(attributes: &Attributes) -> bool {
        attributes.values().all(Self::is_attribute_static)
    }

    /// Whether one attribute and everything nested beneath it is static.
    fn is_attribute_static(attribute: &Attribute) -> bool {
        let source_is_static = match attribute.source() {
            Some(source) => !source.to_string().contains("{{"),
            None => true,
        };

        source_is_static && attribute.mappings().values().all(Self::is_attribute_static)
    }
}

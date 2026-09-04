use cassiopeia_ir::{
    entity::AttributeValues,
    metadata::EntityMetadata,
    relationships::{InstanceRelationships, Relationships},
};

/// The resolved data one entity's attributes are built from.
///
/// The three maps are drained as attributes are built, so whatever is left when the mappings are
/// exhausted is what no declaration claimed; the metadata is read repeatedly and never consumed.
/// They travel together only as far as the dispatch that picks an attribute kind: each builder is
/// handed the one map it draws from, so no builder can reach a store its kind has no business
/// touching.
pub(crate) struct AttributeStore<'a> {
    /// Property-family values, keyed by attribute name.
    pub(crate) values: &'a mut AttributeValues,
    /// Relationship objects, keyed by attribute name.
    pub(crate) relationships: &'a mut Relationships,
    /// Regrouped `ListRelationship` object lists, keyed by attribute name.
    pub(crate) instance_relationships: &'a mut InstanceRelationships,
    /// The attribute-level properties the extractor recorded, if the entity carries any.
    pub(crate) metadata: Option<&'a EntityMetadata>,
}

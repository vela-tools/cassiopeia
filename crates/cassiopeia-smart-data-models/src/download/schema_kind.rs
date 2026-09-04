/// Whether a downloaded schema is an entity model or a support schema referenced by models' `$ref`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    /// An NGSI-LD entity model, selectable as a data model.
    EntityModel,
    /// A support schema (`common-schema`, a per-subject `*-schema`, a `GeoJSON` geometry) that models
    /// reference by `$ref` but that is not itself a data model.
    Support,
}

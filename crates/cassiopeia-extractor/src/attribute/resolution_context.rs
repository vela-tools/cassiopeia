use crate::dropped_geometries::DroppedGeometries;
use cassiopeia_ir::{
    metadata::EntityMetadata,
    relationships::{NestedRelationships, Relationships},
};
use cassiopeia_mapping::template::resolver::TemplateResolver;
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use serde_json::Value as JsonValue;

/// The guard depth for nested attribute resolution. A mapping nested past this is treated as
/// self-referential rather than legitimately deep.
const RECURSION_LIMIT: usize = 50;

/// The state threaded through one entity's attribute resolution.
///
/// A context borrows the source record, the shared template resolver, and the entity's
/// already-resolved relationships (both top-level and nested). It optionally borrows a metadata sink;
/// child contexts, spawned for nested declarations, never collect metadata of their own.
///
/// The two refusal sinks are borrowed shared rather than mutably, and every child context carries
/// the same ones: a nested declaration can be refused as readily as a top-level one, and a batch
/// resolves across a Rayon pool, neither of which a `&mut` sink could cross.
pub(crate) struct ResolutionContext<'a> {
    /// The source record the entity was built from.
    pub(crate) data: &'a JsonValue,
    /// The compiled-template resolver shared across the extraction stage.
    pub(crate) resolver: &'a TemplateResolver,
    /// Relationship targets already resolved for this entity, keyed by attribute name.
    pub(crate) relationships: &'a Relationships,
    /// Objects of nested relationships, keyed by their [`RelationshipPath`] from the entity; `None`
    /// when the entity has none, so the nested-relationship lookup is skipped entirely (ETSI GS CIM
    /// 009 v1.9.1 clause 4.5.2.2 with 4.5.3).
    pub(crate) nested_relationships: Option<&'a NestedRelationships>,
    /// Where a refused geometry conversion is recorded, shared by every context of the batch.
    pub(crate) dropped_geometries: &'a DroppedGeometries,
    /// Where an attribute value that reads as no date-time is recorded, shared by every context of
    /// the batch.
    pub(crate) unreadable_timestamps: &'a UnreadableTimestamps,
    /// How deep into nested attribute declarations this context sits.
    pub(crate) depth: usize,
    /// Where attribute-level properties are recorded, present only on the root context.
    pub(crate) metadata: Option<&'a mut EntityMetadata>,
}

impl<'a> ResolutionContext<'a> {
    /// Builds the root context for one entity's extraction.
    pub(crate) const fn new(
        data: &'a JsonValue,
        resolver: &'a TemplateResolver,
        relationships: &'a Relationships,
        nested_relationships: Option<&'a NestedRelationships>,
        dropped_geometries: &'a DroppedGeometries,
        unreadable_timestamps: &'a UnreadableTimestamps,
        metadata: Option<&'a mut EntityMetadata>,
    ) -> ResolutionContext<'a> {
        ResolutionContext {
            data,
            resolver,
            relationships,
            nested_relationships,
            dropped_geometries,
            unreadable_timestamps,
            depth: 0,
            metadata,
        }
    }

    /// Derives a deeper context over `data`, one level down and collecting no metadata.
    pub(crate) const fn child(&self, data: &'a JsonValue) -> ResolutionContext<'a> {
        ResolutionContext {
            data,
            resolver: self.resolver,
            relationships: self.relationships,
            nested_relationships: self.nested_relationships,
            dropped_geometries: self.dropped_geometries,
            unreadable_timestamps: self.unreadable_timestamps,
            depth: self.depth + 1,
            metadata: None,
        }
    }

    /// Whether resolution has recursed past the guard depth.
    pub(crate) const fn recursion_limit_exceeded(&self) -> bool {
        self.depth > RECURSION_LIMIT
    }
}

use crate::{entity_store::mapping_reads::MappingReads, mapping_id::MappingId, mapping_registry::BoundMapping};
use ahash::AHashMap;
use cassiopeia_mapping::source_reads::SourceReads;

/// Every mapping the registry had handed an id to when it was taken, by that id.
///
/// The registry is a concurrent map the resolver writes while it stores fragments. Assembly only
/// reads it, once per entity and from every thread, so it takes this copy once and reads it without
/// a lock. The copy holds handles on the mappings, not the mappings themselves.
#[derive(Debug, Default)]
pub struct RegisteredMappings(AHashMap<MappingId, BoundMapping>);

impl RegisteredMappings {
    /// Wraps the mappings `bound`, by id.
    pub(crate) fn new(bound: impl IntoIterator<Item = (MappingId, BoundMapping)>) -> RegisteredMappings {
        RegisteredMappings(bound.into_iter().collect())
    }

    /// The mapping `mapping_id` stands for, or `None` for an id never handed out.
    #[must_use]
    pub fn get(&self, mapping_id: MappingId) -> Option<&BoundMapping> {
        self.0.get(&mapping_id)
    }
}

impl MappingReads for RegisteredMappings {
    fn reads(&self, mapping_id: MappingId) -> Option<&SourceReads> {
        self.get(mapping_id).map(|bound| bound.mapping().source_reads())
    }
}

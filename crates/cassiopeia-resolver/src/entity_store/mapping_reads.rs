use crate::mapping_id::MappingId;
use cassiopeia_mapping::source_reads::SourceReads;

/// The read sets of the mappings a store's records came from, by mapping id.
///
/// A store that merges records counts a disagreement between them only beneath a key the mapping
/// reads (see the crate-private `deep_merge`). A write carries its mapping's read set, but a store
/// that finishes a merge at assembly holds only the mapping's id by then, so assembly hands it the
/// read sets to finish it with.
pub trait MappingReads: Sync {
    /// The read set of the mapping `mapping_id`, or `None` for an id no mapping was registered under.
    fn reads(&self, mapping_id: MappingId) -> Option<&SourceReads>;
}

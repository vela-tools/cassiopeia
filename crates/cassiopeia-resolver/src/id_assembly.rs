use crate::{entity_store::discarded_timestamps::DiscardedTimestamps, mapping_id::MappingId, record_merge::RecordMerge};
use cassiopeia_ir::assembled_entity::AssembledEntity;
use cassiopeia_mapping::mapping::Mapping;
use smallvec::SmallVec;
use std::sync::Arc;

/// What a current-state store discarded of one mapping's records resolving to one id, for a loss
/// the run reports.
#[derive(Debug)]
pub struct MappingDiscards {
    /// The interned id of the mapping.
    pub mapping_id: MappingId,
    /// The mapping, which names the declarations a discarded record's unreadable text cost.
    pub mapping: Arc<Mapping>,
    /// The records discarded, by why they had no instant to rank by.
    pub discarded: DiscardedTimestamps,
}

/// The mappings whose records resolving to one id a current-state store discarded for a failing
/// `observedAt` template or unreadable `observedAt` text. One entry at most per contributing
/// mapping, and none in the common case, so the inline capacity keeps it off the heap.
pub type DiscardedRecords = SmallVec<[MappingDiscards; 1]>;

/// Everything assembling one base id yields: the emit-units it streams downstream, the record merge
/// its stored contributions reveal, and the records the store discarded for a failing template.
///
/// Both are read off the same stored fragments the units are built from, while the store hands
/// them over for the one and only time, so it costs no second pass over the store.
#[derive(Debug)]
pub struct IdAssembly {
    /// The id's emit-units: one joined unit for a current-state store, one per observation for a
    /// series store.
    pub units: Vec<AssembledEntity>,
    /// The records of one mapping that resolved to this id and disagreed on a field, when there were
    /// such records.
    pub merge: Option<RecordMerge>,
    /// The records of each mapping the store discarded for a failing `observedAt` template or
    /// unreadable `observedAt` text.
    pub discarded: DiscardedRecords,
}

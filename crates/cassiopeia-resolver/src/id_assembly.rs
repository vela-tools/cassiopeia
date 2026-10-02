use crate::record_merge::RecordMerge;
use cassiopeia_ir::assembled_entity::AssembledEntity;

/// Everything assembling one base id yields: the emit-units it streams downstream, and the record
/// merge its stored contributions reveal.
///
/// The merge is read off the same stored fragments the units are built from, while the store hands
/// them over for the one and only time, so it costs no second pass over the store.
#[derive(Debug)]
pub struct IdAssembly {
    /// The id's emit-units: one joined unit for a current-state store, one per observation for a
    /// series store.
    pub units: Vec<AssembledEntity>,
    /// The records of one mapping that resolved to this id and disagreed on a field, when there were
    /// such records.
    pub merge: Option<RecordMerge>,
}

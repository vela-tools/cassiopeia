use crate::{
    entity_store::{mapping_expiry::MappingExpiry, supersession::SlotRanking},
    field_path::FieldPath,
    mapping_id::MappingId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU64;

/// The on-store representation of an entity fragment.
///
/// `rmp-serde` encodes this as a compact `MessagePack` record so the `mapping_id` and the ranking by
/// record-level `observedAt` live alongside the data on disk. This is an internal store encoding and is unrelated
/// to any ingested data format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFragment {
    /// The id of the mapping that produced the fragment.
    pub mapping_id: MappingId,
    /// The source record the fragment was expanded from.
    pub data: Value,
    /// How a current-state slot ranks the mapping's records by their record-level `observedAt`, kept
    /// across batches together with the failed records it discarded. A series observation carries its
    /// own timestamp and no discards.
    pub ranking: SlotRanking,
    /// How many records were folded into this fragment: one for a series observation, every record
    /// the mapping resolved to the id for a current-state slot.
    pub records: NonZeroU64,
    /// The earliest field on which the folded records disagreed, kept across batches so a conflict
    /// found in one batch is still reported after the next. Always `None` for a series observation.
    pub conflict: Option<FieldPath>,
    /// The entity `expiresAt` the fragment's records contribute: the one folded into a
    /// current-state slot, or the observation's own in a series.
    pub expires_at: MappingExpiry,
}

use crate::mapping_id::MappingId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The on-store representation of an entity fragment.
///
/// `rmp-serde` encodes this as a compact `MessagePack` record so the `mapping_id` and the record-level
/// `observedAt` live alongside the data on disk. This is an internal store encoding and is unrelated
/// to any ingested data format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFragment {
    /// The id of the mapping that produced the fragment.
    pub mapping_id: MappingId,
    /// The source record the fragment was expanded from.
    pub data: Value,
    /// The record-level `observedAt` text, verbatim; used by a current-state store to rank a temporal
    /// mapping's records. `None` for a non-temporal record.
    pub observed_at: Option<String>,
}

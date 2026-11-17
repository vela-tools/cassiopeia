use crate::{entity_store::mapping_expiry::MappingExpiry, mapping_id::MappingId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The on-store representation of one series observation.
///
/// `rmp-serde` encodes this as a compact `MessagePack` record. A series keeps every observation on
/// its own, so nothing about one is ranked, counted or merged, and the record carries only what
/// assembly reads back. This is an internal store encoding and is unrelated to any ingested data
/// format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoredObservation {
    /// The id of the mapping that produced the observation.
    pub(crate) mapping_id: MappingId,
    /// The source record the observation was expanded from.
    pub(crate) data: Value,
    /// The entity `expiresAt` the observation's record declares.
    pub(crate) expires_at: MappingExpiry,
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{mapping_expiry::MappingExpiry, stored_observation::StoredObservation},
        mapping_id::MappingId,
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn an_observation_survives_the_store_encoding() {
        let observation = StoredObservation {
            mapping_id: MappingId::new(3),
            data: json!({"t": 20.5, "time": "2026-03-01T11:00:00Z"}),
            expires_at: MappingExpiry::first(Utc.timestamp_opt(1_800_000_000, 0).single()),
        };
        let encoded = rmp_serde::to_vec(&observation).unwrap();
        assert_eq!(rmp_serde::from_slice::<StoredObservation>(&encoded).unwrap(), observation);
    }
}

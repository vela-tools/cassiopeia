use crate::{
    entity_store::{mapping_expiry::MappingExpiry, store::AssembledFragment, supersession::SlotRanking},
    field_path::FieldPath,
    mapping_id::MappingId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU64;

/// The on-store representation of one mapping's current-state slot for an entity, once every record
/// of the batches that reached it has been merged.
///
/// `rmp-serde` encodes this as a compact `MessagePack` record; the mapping is the key's, so it is not
/// repeated here. A slot is read back into a [`LatestSlot`](crate::entity_store::latest_slot::LatestSlot)
/// to take further records. This is an internal store encoding and is unrelated to any ingested data
/// format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum StoredSlot {
    /// A temporal mapping's record with the latest `observedAt`.
    Latest {
        /// How many of the mapping's records resolved to the id.
        records: NonZeroU64,
        /// The retained record.
        data: Value,
        /// How the slot ranks the mapping's records, with the failed records it discarded.
        ranking: SlotRanking,
        /// The entity `expiresAt` of the retained record.
        expires_at: MappingExpiry,
    },
    /// A static mapping's records, merged in source order.
    Merged {
        /// How many of the mapping's records resolved to the id.
        records: NonZeroU64,
        /// The merged record.
        data: Value,
        /// The earliest field the mapping reads on which the merged records disagreed.
        conflict: Option<FieldPath>,
        /// The entity `expiresAt` the merged records contribute.
        expires_at: MappingExpiry,
    },
}

impl StoredSlot {
    /// The slot as the fragment of the mapping `mapping_id` in the assembled entity, with the expiry
    /// its records contribute.
    pub(crate) fn into_fragment(self, mapping_id: MappingId) -> (AssembledFragment, MappingExpiry) {
        let (records, data, conflict, expires_at, unresolvable_discards) = match self {
            StoredSlot::Latest {
                records,
                data,
                ranking,
                expires_at,
            } => (records, data, None, expires_at, ranking.unresolvable_discards()),
            StoredSlot::Merged {
                records,
                data,
                conflict,
                expires_at,
            } => (records, data, conflict, expires_at, 0),
        };
        let fragment = AssembledFragment {
            mapping_id,
            data,
            records,
            conflict,
            unresolvable_discards,
        };
        (fragment, expires_at)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{mapping_expiry::MappingExpiry, record_timestamp::RecordTimestamp, stored_slot::StoredSlot, supersession::SlotRanking},
        field_path::FieldPath,
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use cassiopeia_mapping::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::num::NonZeroU64;

    #[test]
    fn either_kind_of_slot_survives_the_store_encoding() {
        let slots = [
            StoredSlot::Latest {
                records: NonZeroU64::new(3).unwrap(),
                data: json!({"t": 21.5}),
                ranking: SlotRanking::first(
                    RecordTimestamp::Declared(ObservedAt::new("2026-03-01T11:00:00Z")),
                    SourcePosition::first_of(1).advanced(7),
                ),
                expires_at: MappingExpiry::first(None),
            },
            StoredSlot::Merged {
                records: NonZeroU64::new(2).unwrap(),
                data: json!({"name": "Main", "capacity": 40}),
                conflict: Some(FieldPath::new(&["capacity"])),
                expires_at: MappingExpiry::first(None),
            },
        ];
        for slot in slots {
            let encoded = rmp_serde::to_vec(&slot).unwrap();
            assert_eq!(rmp_serde::from_slice::<StoredSlot>(&encoded).unwrap(), slot);
        }
    }
}

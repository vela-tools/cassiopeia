use crate::{
    entity_store::{discarded_timestamps::DiscardedTimestamps, mapping_expiry::MappingExpiry, ranked_records::RankedRecords, store::AssembledFragment},
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
    /// A temporal mapping's records, each kept by the rankings it holds.
    Ranked {
        /// How many of the mapping's records resolved to the id.
        records: NonZeroU64,
        /// The records kept, how each ranking holds them, and the losses of those released.
        ranked: RankedRecords,
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
        match self {
            StoredSlot::Ranked { records, ranked } => ranked.into_fragment(mapping_id, records),
            StoredSlot::Merged {
                records,
                data,
                conflict,
                expires_at,
            } => {
                let fragment = AssembledFragment {
                    mapping_id,
                    data,
                    observed: None,
                    records,
                    conflict,
                    discarded: DiscardedTimestamps::default(),
                };
                (fragment, expires_at)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            mapping_expiry::MappingExpiry,
            observations::Observations,
            ranked_records::{ObservedRecord, RankedRecords},
            record_timestamp::RecordTimestamp,
            stored_slot::StoredSlot,
            supersession::RecordOrdinal,
        },
        field_path::FieldPath,
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use cassiopeia_mapping::observed_template::ObservedTemplateIndex;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::num::NonZeroU64;

    /// The record `data`, the `count`-th of its mapping, observed at `a` and `b`, each its own slot.
    fn observed(data: Value, count: u64, [a, b]: [&str; 2]) -> ObservedRecord {
        ObservedRecord {
            data,
            position: SourcePosition::first_of(1).advanced(usize::try_from(count).unwrap()),
            record: RecordOrdinal::new(NonZeroU64::new(count).unwrap()),
            observations: Observations::of(
                [RecordTimestamp::read(json!(a)), RecordTimestamp::read(json!(b))],
                &[ObservedTemplateIndex::new(0), ObservedTemplateIndex::new(1)],
            ),
            expires_at: None,
        }
    }

    #[test]
    fn either_kind_of_slot_survives_the_store_encoding() {
        let mut ranked = RankedRecords::first(observed(json!({"a": 1, "b": 1}), 1, ["2026-03-01T11:00:00Z", "2026-03-01T09:00:00Z"]));
        ranked.absorb(observed(json!({"a": 2, "b": 2}), 2, ["2026-03-01T10:00:00Z", "soon"]));
        ranked.absorb(observed(json!({"a": 3, "b": 3}), 3, ["2026-03-01T10:00:00Z", "2026-03-01T12:00:00Z"]));
        let slots = [
            StoredSlot::Ranked {
                records: NonZeroU64::new(3).unwrap(),
                ranked,
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

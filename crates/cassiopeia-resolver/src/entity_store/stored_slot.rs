use crate::{
    entity_store::{
        discarded_timestamps::DiscardedTimestamps,
        input_merge::{InputMerges, join},
        mapping_expiry::MappingExpiry,
        ranked_records::RankedRecords,
        store::AssembledFragment,
    },
    mapping_id::MappingId,
};
use cassiopeia_mapping::source_reads::SourceReads;
use std::num::NonZeroU64;

/// One mapping's current-state slot for an entity, once every record of the batches that reached it
/// has been merged into the merge of its input.
///
/// A slot is read back into a [`LatestSlot`](crate::entity_store::latest_slot::LatestSlot) to take
/// further records. The disk-backed store keeps it on disk as an
/// [`EncodedSlot`](crate::entity_store::redb_slot_value::EncodedSlot), with each input's merge encoded
/// on its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StoredSlot {
    /// A temporal mapping's records, each kept by the rankings it holds.
    Ranked {
        /// How many of the mapping's records resolved to the id.
        records: NonZeroU64,
        /// The records kept, how each ranking holds them, and the losses of those released.
        ranked: RankedRecords,
    },
    /// A static mapping's records, each input's merged in the order of the input.
    Merged {
        /// How many of the mapping's records resolved to the id, over every input.
        records: NonZeroU64,
        /// Each input's records, merged, in the order of the inputs in the manifest.
        inputs: InputMerges,
    },
}

impl StoredSlot {
    /// The slot as the fragment of the mapping `mapping_id` in the assembled entity, with the expiry
    /// its records contribute.
    ///
    /// A static mapping's inputs are joined in the order of the manifest, counting a disagreement
    /// between them only beneath a key in `reads`, the mapping's read set: no record of an earlier
    /// input can still arrive once the entity is assembled.
    pub(crate) fn into_fragment(self, mapping_id: MappingId, reads: &SourceReads) -> (AssembledFragment, MappingExpiry) {
        match self {
            StoredSlot::Ranked { records, ranked } => ranked.into_fragment(mapping_id, records),
            StoredSlot::Merged { records, inputs } => {
                let (data, conflict, expires_at) = join(inputs, reads);
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
        entity_store::{input_merge::InputMerge, mapping_expiry::MappingExpiry, stored_slot::StoredSlot},
        field_path::FieldPath,
        mapping_id::MappingId,
    };
    use cassiopeia_mapping::source_reads::SourceReads;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use smallvec::smallvec;
    use std::num::NonZeroU64;

    /// The merge of `records`, in order, which the input at `input` in the manifest gave.
    fn input(input: u32, records: &[Value]) -> InputMerge {
        let mut merge = InputMerge::nothing(input);
        for record in records {
            merge.absorb(InputMerge::record(input, record.clone(), None), &SourceReads::Everything);
        }
        merge
    }

    #[test]
    fn a_static_slot_joins_its_inputs_in_the_order_of_the_manifest_into_one_fragment() {
        let slot = StoredSlot::Merged {
            records: NonZeroU64::new(3).unwrap(),
            inputs: smallvec![
                input(0, &[json!({"name": "Main", "capacity": 40})]),
                input(1, &[json!({"capacity": 42}), json!({"capacity": 43, "zone": "B"})]),
            ],
        };

        let (fragment, expiry) = slot.into_fragment(MappingId::new(1), &SourceReads::Everything);

        assert_eq!(
            (fragment.data, fragment.records.get(), fragment.conflict, expiry),
            (
                json!({"name": "Main", "capacity": 40, "zone": "B"}),
                3,
                Some(FieldPath::new(&["capacity"])),
                MappingExpiry::first(None)
            )
        );
    }
}

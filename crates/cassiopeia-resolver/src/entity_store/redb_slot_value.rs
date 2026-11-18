use crate::entity_store::{
    error::{EntityStoreError, Result},
    input_merge::InputMerge,
    ranked_records::RankedRecords,
    stored_slot::StoredSlot,
};
use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
use rmp_serde::encode::write;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use smallvec::{SmallVec, smallvec};
use std::num::NonZeroU64;

/// One mapping's slot for an id as the disk-backed current-state store encodes it: a [`StoredSlot`]
/// whose static mapping keeps the merge of its one input inline, as the slot of a run of one input
/// always does, or each of several inputs' merges encoded on its own.
///
/// `rmp-serde` encodes this as a compact `MessagePack` record; the mapping is the key's, so it is not
/// repeated here. This is an internal store encoding and is unrelated to any ingested data format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum EncodedSlot {
    /// A temporal mapping's records, each kept by the rankings it holds.
    Ranked {
        /// How many of the mapping's records resolved to the id.
        records: NonZeroU64,
        /// The records kept, how each ranking holds them, and the losses of those released.
        ranked: RankedRecords,
    },
    /// A static mapping's records, all from one input, merged in the order of the input.
    OneInput {
        /// How many of the mapping's records resolved to the id.
        records: NonZeroU64,
        /// The input's records, merged.
        merge: InputMerge,
    },
    /// A static mapping's records from several inputs, each input's merged in the order of the input
    /// and encoded.
    Inputs {
        /// How many of the mapping's records resolved to the id, over every input.
        records: NonZeroU64,
        /// Each input's merge, encoded, in the order of the inputs in the manifest.
        inputs: EncodedInputs,
    },
}

/// The encoded merges of a static mapping's inputs, in the order of the inputs. Up to four inputs are
/// held without a heap allocation, which also keeps a slot of several inputs about as large as a
/// temporal one.
pub(crate) type EncodedInputs = SmallVec<[EncodedInput; 4]>;

/// One input's merge of a static mapping's records as the disk-backed store keeps it: the input's
/// place in the manifest beside the merge, encoded on its own.
///
/// A batch decodes the merge of an input only once a record of that input reaches the slot, and
/// writes the merges of the other inputs back as it read them, so a slot that several inputs share
/// costs a write about what a slot of one input does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct EncodedInput {
    /// The place in the manifest of the input that read the records.
    input: u32,
    /// The input's merge, encoded with `rmp-serde`, kept as `MessagePack` binary rather than an
    /// array of numbers.
    merge: ByteBuf,
}

impl EncodedInput {
    /// The merge `merge`, encoded.
    pub(crate) fn encode(merge: &InputMerge) -> Result<EncodedInput> {
        Ok(EncodedInput {
            input: merge.input(),
            merge: ByteBuf::from(rmp_serde::to_vec(merge)?),
        })
    }

    /// The place in the manifest of the input that read the records.
    pub(crate) const fn input(&self) -> u32 {
        self.input
    }

    /// The merge, decoded.
    pub(crate) fn decode(&self) -> Result<InputMerge> {
        Ok(rmp_serde::from_slice(&self.merge)?)
    }
}

/// The slot `encoded` with every input's merge decoded, as assembly needs it.
pub(crate) fn decoded(encoded: EncodedSlot) -> Result<StoredSlot> {
    match encoded {
        EncodedSlot::Ranked { records, ranked } => Ok(StoredSlot::Ranked { records, ranked }),
        EncodedSlot::OneInput { records, merge } => Ok(StoredSlot::Merged {
            records,
            inputs: smallvec![merge],
        }),
        EncodedSlot::Inputs { records, inputs } => Ok(StoredSlot::Merged {
            records,
            inputs: inputs.iter().map(EncodedInput::decode).collect::<Result<_>>()?,
        }),
    }
}

/// The slot `encoded` with the merges of several inputs left encoded beside it, for a batch to decode
/// those of the inputs its records come from. The merge of a slot's one input is held decoded.
pub(crate) fn undecoded(encoded: EncodedSlot) -> (StoredSlot, EncodedInputs) {
    match encoded {
        EncodedSlot::Ranked { records, ranked } => (StoredSlot::Ranked { records, ranked }, SmallVec::new()),
        EncodedSlot::OneInput { records, merge } => (
            StoredSlot::Merged {
                records,
                inputs: smallvec![merge],
            },
            SmallVec::new(),
        ),
        EncodedSlot::Inputs { records, inputs } => (
            StoredSlot::Merged {
                records,
                inputs: SmallVec::new(),
            },
            inputs,
        ),
    }
}

/// The slot `slot` a batch took records into, joined by `untouched`, the merges of the inputs no
/// record of the batch reached, as they were read: the merge of the slot's one input inline, or every
/// input's merge encoded, in the order of the manifest.
///
/// A temporal mapping's slot keeps no input apart, so `untouched` is empty beside one.
pub(crate) fn joined(slot: StoredSlot, untouched: EncodedInputs) -> Result<EncodedSlot> {
    match slot {
        StoredSlot::Ranked { records, ranked } => Ok(EncodedSlot::Ranked { records, ranked }),
        StoredSlot::Merged { records, mut inputs } => {
            if untouched.is_empty()
                && inputs.len() == 1
                && let Some(merge) = inputs.pop()
            {
                return Ok(EncodedSlot::OneInput { records, merge });
            }
            let mut encoded = untouched;
            for merge in &inputs {
                encoded.push(EncodedInput::encode(merge)?);
            }
            if !encoded.is_sorted_by_key(EncodedInput::input) {
                encoded.sort_by_key(EncodedInput::input);
            }
            Ok(EncodedSlot::Inputs { records, inputs: encoded })
        }
    }
}

/// One mapping's slot for an id as the disk-backed current-state store keeps it: the position of the
/// first record the mapping resolved to the id, as its big-endian bytes, then the slot in
/// `MessagePack`.
///
/// The position leads the value so a batch reaching an id learns where the id's first record stands
/// from every slot of the id while decoding only the slots its own records land on. This is an
/// internal store encoding and is unrelated to any ingested data format.
pub(crate) fn encode_slot(first: SourcePosition, slot: &EncodedSlot) -> Result<Vec<u8>> {
    let mut value = Vec::from(first.to_be_bytes());
    write(&mut value, slot)?;
    Ok(value)
}

/// The position of the first record of the slot [`encode_slot`] wrote as `value`.
pub(crate) fn slot_first(value: &[u8]) -> Result<SourcePosition> {
    split(value).map(|(first, _slot)| first)
}

/// The slot [`encode_slot`] wrote as `value`, with the position of its first record.
pub(crate) fn decode_slot(value: &[u8]) -> Result<(SourcePosition, EncodedSlot)> {
    let (first, slot) = split(value)?;
    Ok((first, rmp_serde::from_slice(slot)?))
}

/// The position `value` leads with, and the encoded slot after it.
fn split(value: &[u8]) -> Result<(SourcePosition, &[u8])> {
    let (first, slot) = value
        .split_first_chunk::<SOURCE_POSITION_BYTES>()
        .ok_or(EntityStoreError::TruncatedPosition { length: value.len() })?;
    Ok((SourcePosition::from_be_bytes(*first), slot))
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        error::EntityStoreError,
        input_merge::InputMerge,
        observations::Observations,
        ranked_records::{ObservedRecord, RankedRecords},
        record_timestamp::RecordTimestamp,
        redb_slot_value::{EncodedInput, EncodedSlot, decode_slot, decoded, encode_slot, joined, slot_first, undecoded},
        stored_slot::StoredSlot,
        supersession::RecordOrdinal,
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use cassiopeia_mapping::{observed_template::ObservedTemplateIndex, source_reads::SourceReads};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use smallvec::smallvec;
    use std::num::NonZeroU64;

    /// The merge of `record` alone, which the input at `input` in the manifest gave.
    fn merge(input: u32, record: Value) -> InputMerge {
        let mut merge = InputMerge::nothing(input);
        merge.absorb(InputMerge::record(input, record, None), &SourceReads::Everything);
        merge
    }

    fn slot() -> EncodedSlot {
        EncodedSlot::Inputs {
            records: NonZeroU64::MIN.saturating_add(2),
            inputs: smallvec![
                EncodedInput::encode(&merge(0, json!({"name": "Main"}))).unwrap(),
                EncodedInput::encode(&merge(2, json!({"capacity": 42}))).unwrap(),
            ],
        }
    }

    #[test]
    fn a_slot_reads_back_with_the_position_of_its_first_record() {
        let first = SourcePosition::first_of(2).advanced(41).nth_fragment(1);
        let value = encode_slot(first, &slot()).unwrap();

        assert_eq!(slot_first(&value).unwrap(), first);
        assert_eq!(decode_slot(&value).unwrap(), (first, slot()));
    }

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
        let first = SourcePosition::first_of(1);
        for slot in [
            EncodedSlot::Ranked {
                records: NonZeroU64::new(3).unwrap(),
                ranked,
            },
            EncodedSlot::OneInput {
                records: NonZeroU64::MIN,
                merge: merge(0, json!({"name": "Main"})),
            },
            slot(),
        ] {
            let value = encode_slot(first, &slot).unwrap();
            assert_eq!(decode_slot(&value).unwrap(), (first, slot));
        }
    }

    #[test]
    fn a_value_too_short_to_hold_a_position_is_refused() {
        let value = [0_u8; 15];

        assert!(matches!(slot_first(&value), Err(EntityStoreError::TruncatedPosition { length: 15 })));
        assert!(matches!(decode_slot(&value), Err(EntityStoreError::TruncatedPosition { length: 15 })));
    }

    #[test]
    fn an_input_s_merge_reads_back_from_its_encoding() {
        let merge = merge(3, json!({"capacity": 40, "zone": "B"}));

        let encoded = EncodedInput::encode(&merge).unwrap();

        assert_eq!(encoded.input(), 3);
        assert_eq!(encoded.decode().unwrap(), merge);
    }

    #[test]
    fn a_slot_decodes_every_input_s_merge_for_assembly() {
        assert_eq!(
            decoded(slot()).unwrap(),
            StoredSlot::Merged {
                records: NonZeroU64::MIN.saturating_add(2),
                inputs: smallvec![merge(0, json!({"name": "Main"})), merge(2, json!({"capacity": 42}))],
            }
        );
    }

    #[test]
    fn a_slot_of_one_input_keeps_its_merge_inline_and_hands_it_back_decoded() {
        let taken = StoredSlot::Merged {
            records: NonZeroU64::MIN.saturating_add(1),
            inputs: smallvec![merge(3, json!({"capacity": 40}))],
        };

        let encoded = joined(taken.clone(), smallvec![]).unwrap();

        assert_eq!(
            encoded,
            EncodedSlot::OneInput {
                records: NonZeroU64::MIN.saturating_add(1),
                merge: merge(3, json!({"capacity": 40})),
            }
        );
        let (stored, untouched) = undecoded(encoded.clone());
        assert_eq!((stored, untouched.is_empty()), (taken.clone(), true));
        assert_eq!(decoded(encoded).unwrap(), taken);
    }

    #[test]
    fn a_second_input_reaching_a_slot_of_one_input_encodes_both_merges() {
        let (stored, untouched) = undecoded(EncodedSlot::OneInput {
            records: NonZeroU64::MIN,
            merge: merge(1, json!({"capacity": 42})),
        });
        let StoredSlot::Merged { records, mut inputs } = stored else {
            panic!("a static slot reads back as a static slot");
        };
        inputs.insert(0, merge(0, json!({"capacity": 40})));

        let encoded = joined(
            StoredSlot::Merged {
                records: records.saturating_add(1),
                inputs,
            },
            untouched,
        )
        .unwrap();

        assert_eq!(
            encoded,
            EncodedSlot::Inputs {
                records: NonZeroU64::MIN.saturating_add(1),
                inputs: smallvec![
                    EncodedInput::encode(&merge(0, json!({"capacity": 40}))).unwrap(),
                    EncodedInput::encode(&merge(1, json!({"capacity": 42}))).unwrap(),
                ],
            }
        );
    }

    #[test]
    fn a_batch_writes_back_the_inputs_it_left_encoded_beside_those_it_took_records_into_in_the_order_of_the_inputs() {
        let (stored, untouched) = undecoded(slot());
        assert_eq!(
            stored,
            StoredSlot::Merged {
                records: NonZeroU64::MIN.saturating_add(2),
                inputs: smallvec![],
            }
        );
        assert_eq!(untouched.iter().map(EncodedInput::input).collect::<Vec<u32>>(), [0, 2]);

        let taken = StoredSlot::Merged {
            records: NonZeroU64::MIN.saturating_add(3),
            inputs: smallvec![merge(1, json!({"capacity": 41}))],
        };

        assert_eq!(
            decoded(joined(taken, untouched).unwrap()).unwrap(),
            StoredSlot::Merged {
                records: NonZeroU64::MIN.saturating_add(3),
                inputs: smallvec![
                    merge(0, json!({"name": "Main"})),
                    merge(1, json!({"capacity": 41})),
                    merge(2, json!({"capacity": 42})),
                ],
            }
        );
    }
}

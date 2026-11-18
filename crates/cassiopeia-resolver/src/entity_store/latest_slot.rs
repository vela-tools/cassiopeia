use crate::entity_store::{
    input_merge::{InputMerge, InputMerges},
    observations::RecordRanking,
    ranked_records::{ObservedRecord, RankedRecords},
    stored_slot::StoredSlot,
    supersession::RecordOrdinal,
    write_batch::WriteBatch,
};
use cassiopeia_ir::source_position::SourcePosition;
use cassiopeia_mapping::source_reads::SourceReads;
use chrono::{DateTime, Utc};
use serde_json::Value;
use smallvec::{SmallVec, smallvec};
use std::num::NonZeroU64;

/// One record on its way into a slot.
pub(crate) struct IncomingRecord {
    /// The source record.
    pub(crate) data: Value,
    /// Where the record stands in the source.
    pub(crate) position: SourcePosition,
    /// The batch of resolver writes the record arrived in.
    pub(crate) batch: WriteBatch,
    /// What the record ranks by, when its mapping ranks its records.
    pub(crate) ranking: RecordRanking,
    /// The entity `expiresAt` the record declares.
    pub(crate) expires_at: Option<DateTime<Utc>>,
}

/// One mapping's slot for a base id in a current-state store: how many of the mapping's records
/// resolved to the id, and what the slot keeps of them.
///
/// Both current-state stores apply their rules through this type: the in-memory store keeps its slots
/// as it, and the disk-backed store reads a [`StoredSlot`] into one for each batch that touches it and
/// writes it back sealed.
#[derive(Debug)]
pub(crate) struct LatestSlot {
    /// How many of the mapping's records resolved to the id, whether merged, superseding, or
    /// discarded as stale.
    records: NonZeroU64,
    /// What the slot keeps of them.
    kept: Kept,
}

/// What a slot keeps of its mapping's records, by the rule the mapping's kind sets.
#[derive(Debug)]
enum Kept {
    /// A temporal mapping keeps, attribute by attribute, the records observed last.
    Ranked(RankedRecords),
    /// A static mapping merges its records.
    Merged(MergedRecords),
}

/// A static mapping's records, merged input by input.
///
/// A run reads its inputs side by side, so the batches of two inputs that share a mapping reach the
/// store interleaved in whatever order they are ready, while the batches of one input reach it in the
/// order of the input. Each input's records are therefore merged apart, and the inputs are joined in
/// the order of the manifest at assembly, once no record of an earlier input can still arrive (see
/// [`join`](crate::entity_store::input_merge::join)).
#[derive(Debug)]
struct MergedRecords {
    /// The records of each input that gave the mapping records for the id, in the order of the inputs
    /// in the manifest. One input in the common case of an id that only one input names, which is
    /// held without a heap allocation; smallvec's `union` layout tells the inline input from a spilled
    /// list by the capacity alone, so the list costs one word beside it.
    inputs: SmallVec<[InputRecords; 1]>,
}

/// The records one input gave a static mapping for the id, merged in the order of the input.
///
/// Which value a field the records disagree on keeps depends on the order they are merged in, and
/// the records of one batch reach the store in whatever order the resolver's threads finish them.
/// The input's records of the latest batch are therefore held apart and merged in source order once
/// a write of a later batch, or assembly, shows that none of the batch's records is still to come.
/// Every earlier batch of the input came before the batch held apart, so merging batch by batch keeps
/// the order of the input across batches too.
#[derive(Debug)]
struct InputRecords {
    /// The input's records of every batch before `batch`, merged.
    merged: InputMerge,
    /// The batch `pending` holds records of.
    batch: WriteBatch,
    /// The input's records of `batch`, not merged yet. One record in the common case of an entity
    /// with one record in a batch, which is held without a heap allocation.
    pending: SmallVec<[PendingRecord; 1]>,
}

/// A static mapping's record held apart until the rest of its batch has arrived.
#[derive(Debug)]
struct PendingRecord {
    /// Where the record stands in the source.
    position: SourcePosition,
    /// The source record.
    data: Value,
    /// The entity `expiresAt` the record declares.
    expires_at: Option<DateTime<Utc>>,
}

impl LatestSlot {
    /// A slot holding the mapping's first record for the id, kept as a temporal mapping keeps its
    /// records when the record is ranked, or as a static one does.
    pub(crate) fn new(incoming: IncomingRecord) -> LatestSlot {
        let records = NonZeroU64::MIN;
        let kept = match incoming.ranking {
            RecordRanking::Ranked(observations) => Kept::Ranked(RankedRecords::first(ObservedRecord {
                data: incoming.data,
                position: incoming.position,
                record: RecordOrdinal::new(records),
                observations,
                expires_at: incoming.expires_at,
            })),
            RecordRanking::Unranked => Kept::Merged(MergedRecords {
                inputs: smallvec![InputRecords::first(incoming)],
            }),
        };
        LatestSlot { records, kept }
    }

    /// Folds a further record of the mapping into the slot and counts it. A static mapping's
    /// disagreements count only beneath a key in `reads`.
    pub(crate) fn absorb(&mut self, incoming: IncomingRecord, reads: &SourceReads) {
        self.records = self.records.saturating_add(1);
        match &mut self.kept {
            // A temporal mapping's records are observations of one id: each attribute keeps the record
            // observed last, expiry and all.
            Kept::Ranked(ranked) => ranked.absorb(ObservedRecord {
                data: incoming.data,
                position: incoming.position,
                record: RecordOrdinal::new(self.records),
                observations: incoming.ranking.into_observations(),
                expires_at: incoming.expires_at,
            }),
            Kept::Merged(merged) => merged.absorb(incoming, reads),
        }
    }

    /// The slot `stored` holds, in the batch `batch` reads it back for.
    pub(crate) fn from_stored(stored: StoredSlot, batch: WriteBatch) -> LatestSlot {
        match stored {
            StoredSlot::Ranked { records, ranked } => LatestSlot {
                records,
                kept: Kept::Ranked(ranked),
            },
            StoredSlot::Merged { records, inputs } => LatestSlot {
                records,
                kept: Kept::Merged(MergedRecords {
                    inputs: inputs
                        .into_iter()
                        .map(|merged| InputRecords {
                            merged,
                            batch,
                            pending: SmallVec::new(),
                        })
                        .collect(),
                }),
            },
        }
    }

    /// Takes back `merge`, the merge of one input's records that the store kept apart from the slot
    /// since an earlier batch, ahead of a further record of that input in the batch `batch`.
    ///
    /// A temporal mapping's slot ranks the records of every input together and keeps no input apart,
    /// so it has no merge to take back.
    pub(crate) fn restore_input(&mut self, merge: InputMerge, batch: WriteBatch) {
        match &mut self.kept {
            Kept::Ranked(_) => {}
            Kept::Merged(merged) => merged.restore(merge, batch),
        }
    }

    /// The slot as it is stored once every record of its batch has reached it, merging the records it
    /// holds apart with `reads`.
    pub(crate) fn into_stored(self, reads: &SourceReads) -> StoredSlot {
        let LatestSlot { records, kept } = self;
        match kept {
            Kept::Ranked(ranked) => StoredSlot::Ranked { records, ranked },
            Kept::Merged(merged) => StoredSlot::Merged {
                records,
                inputs: merged.sealed(reads),
            },
        }
    }
}

impl MergedRecords {
    /// Folds a further record into the records of its input, or starts the input's records with it,
    /// in its place among the inputs.
    fn absorb(&mut self, incoming: IncomingRecord, reads: &SourceReads) {
        let input = incoming.position.input();
        let place = self.inputs.partition_point(|known| known.merged.input() < input);
        match self.inputs.get_mut(place) {
            Some(known) if known.merged.input() == input => known.absorb(incoming, reads),
            Some(_) | None => self.inputs.insert(place, InputRecords::first(incoming)),
        }
    }

    /// Takes back the merge of an input's earlier records, in its place among the inputs, to take
    /// further records of the input in the batch `batch`.
    fn restore(&mut self, merged: InputMerge, batch: WriteBatch) {
        let place = self.inputs.partition_point(|known| known.merged.input() < merged.input());
        self.inputs.insert(
            place,
            InputRecords {
                merged,
                batch,
                pending: SmallVec::new(),
            },
        );
    }

    /// Every input's records merged, the records held apart included, in the order of the inputs.
    fn sealed(self, reads: &SourceReads) -> InputMerges {
        self.inputs.into_iter().map(|input| input.sealed(reads)).collect()
    }
}

impl InputRecords {
    /// The records of the input of `incoming`, holding it alone.
    fn first(incoming: IncomingRecord) -> InputRecords {
        InputRecords {
            merged: InputMerge::nothing(incoming.position.input()),
            batch: incoming.batch,
            pending: smallvec![PendingRecord::from(incoming)],
        }
    }

    /// Holds a further record of the input apart until the rest of its batch has arrived.
    fn absorb(&mut self, incoming: IncomingRecord, reads: &SourceReads) {
        // The resolver writes every record of a batch before any of the next, and the records of one
        // input in the order of the input, so a record of a later batch means the held records are
        // all there.
        if incoming.batch != self.batch {
            self.seal(reads);
            self.batch = incoming.batch;
        }
        self.pending.push(PendingRecord::from(incoming));
    }

    /// Merges the held records into the merged ones in source order, counting a disagreement only
    /// beneath a key in `reads`.
    fn seal(&mut self, reads: &SourceReads) {
        // Stable, so two records sharing a position, which only one record expanding into the same
        // entity twice gives, keep the order they arrived in.
        if !self.pending.is_sorted_by_key(|record| record.position) {
            self.pending.sort_by_key(|record| record.position);
        }
        for record in self.pending.drain(..) {
            let input = self.merged.input();
            self.merged.absorb(InputMerge::record(input, record.data, record.expires_at), reads);
        }
    }

    /// The input's records, every one of them merged.
    fn sealed(mut self, reads: &SourceReads) -> InputMerge {
        self.seal(reads);
        self.merged
    }
}

impl From<IncomingRecord> for PendingRecord {
    fn from(incoming: IncomingRecord) -> PendingRecord {
        PendingRecord {
            position: incoming.position,
            data: incoming.data,
            expires_at: incoming.expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            input_merge::InputMerge,
            latest_slot::{IncomingRecord, LatestSlot, MergedRecords, PendingRecord},
            mapping_expiry::MappingExpiry,
            observations::RecordRanking,
            store::tests::ranked,
            stored_slot::StoredSlot,
            write_batch::{WriteBatch, WriteBatches},
        },
        field_path::FieldPath,
        mapping_id::MappingId,
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use cassiopeia_mapping::source_reads::SourceReads;
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use smallvec::SmallVec;
    use std::{mem, num::NonZeroU64};

    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    /// The position of record `record` of the input at `input` in the manifest.
    fn of_input(input: u32, record: usize) -> SourcePosition {
        SourcePosition::first_of(input).advanced(record)
    }

    fn instant(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn incoming(data: Value, position: SourcePosition, batch: WriteBatch) -> IncomingRecord {
        IncomingRecord {
            data,
            position,
            batch,
            ranking: RecordRanking::Unranked,
            expires_at: None,
        }
    }

    /// The fragment a static slot fed `records`, in order, assembles into.
    fn merged(records: Vec<IncomingRecord>) -> (Value, Option<String>, MappingExpiry) {
        let mut records = records.into_iter();
        let mut slot = LatestSlot::new(records.next().unwrap());
        for record in records {
            slot.absorb(record, &SourceReads::Everything);
        }
        let (fragment, expiry) = slot
            .into_stored(&SourceReads::Everything)
            .into_fragment(MappingId::new(1), &SourceReads::Everything);
        (fragment.data, fragment.conflict.map(|path| path.to_string()), expiry)
    }

    #[test]
    fn records_of_one_batch_merge_in_source_order_whatever_order_they_arrive_in() {
        let batches = WriteBatches::new();
        let batch = batches.next();
        let records = || {
            [
                (json!({"capacity": 40, "name": "Main"}), at(0)),
                (json!({"capacity": 42}), at(1)),
                (json!({"capacity": 44, "name": "Renamed", "zone": "B"}), at(2)),
            ]
        };
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let arrivals = order.iter().map(|index| {
                let (data, position) = records()[*index].clone();
                incoming(data, position, batch)
            });
            assert_eq!(
                merged(arrivals.collect()),
                (
                    json!({"capacity": 40, "name": "Main", "zone": "B"}),
                    Some("capacity".to_string()),
                    MappingExpiry::first(None)
                ),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_later_batch_merges_after_every_record_of_an_earlier_one() {
        let batches = WriteBatches::new();
        let (first, second) = (batches.next(), batches.next());

        let (data, conflict, _) = merged(vec![
            incoming(json!({"capacity": 41}), at(1), first),
            incoming(json!({"capacity": 40}), at(0), first),
            incoming(json!({"capacity": 50, "zone": "C"}), at(5), second),
        ]);

        assert_eq!((data, conflict), (json!({"capacity": 40, "zone": "C"}), Some("capacity".to_string())));
    }

    #[test]
    fn a_merged_record_that_declares_an_expiry_overrides_the_ones_before_it_in_source_order() {
        let batch = WriteBatches::new().next();
        let expiring = |data: Value, position, seconds: Option<i64>| IncomingRecord {
            expires_at: seconds.map(instant),
            ..incoming(data, position, batch)
        };

        let (_, _, expiry) = merged(vec![
            expiring(json!({"a": 3}), at(3), None),
            expiring(json!({"a": 2}), at(2), Some(200)),
            expiring(json!({"a": 1}), at(1), Some(100)),
        ]);

        assert_eq!(expiry, MappingExpiry::first(Some(instant(200))));
    }

    #[test]
    fn a_slot_read_back_from_the_store_takes_further_records_by_the_same_rules() {
        let batches = WriteBatches::new();
        let mut slot = LatestSlot::new(incoming(json!({"capacity": 40, "name": "Main"}), at(0), batches.next()));
        slot.absorb(incoming(json!({"capacity": 41}), at(1), batches.next()), &SourceReads::Everything);
        let stored = slot.into_stored(&SourceReads::Everything);

        let later = batches.next();
        let mut read_back = LatestSlot::from_stored(stored, later);
        read_back.absorb(incoming(json!({"capacity": 42, "zone": "C"}), at(3), later), &SourceReads::Everything);
        read_back.absorb(incoming(json!({"capacity": 43}), at(2), later), &SourceReads::Everything);
        let (fragment, _) = read_back
            .into_stored(&SourceReads::Everything)
            .into_fragment(MappingId::new(1), &SourceReads::Everything);

        assert_eq!(
            (fragment.data, fragment.records.get(), fragment.conflict.map(|path| path.to_string())),
            (json!({"capacity": 40, "name": "Main", "zone": "C"}), 4, Some("capacity".to_string()))
        );
    }

    #[test]
    fn a_temporal_slot_keeps_the_latest_record_whichever_order_records_arrive_in() {
        let batch = WriteBatches::new().next();
        let observed = |data: Value, position, time: &str| IncomingRecord {
            ranking: ranked(time),
            ..incoming(data, position, batch)
        };
        let mut slot = LatestSlot::new(observed(json!({"t": 3}), at(3), "2026-03-01T11:00:00Z"));
        slot.absorb(observed(json!({"t": 1}), at(1), "2026-03-01T11:00:00Z"), &SourceReads::Everything);
        slot.absorb(observed(json!({"t": 0}), at(0), "2026-03-01T10:00:00Z"), &SourceReads::Everything);

        let (fragment, _) = slot
            .into_stored(&SourceReads::Everything)
            .into_fragment(MappingId::new(1), &SourceReads::Everything);
        assert_eq!((fragment.data, fragment.records.get()), (json!({"t": 3}), 3));
    }

    /// The fragment a static slot assembles into once each of `batches` has reached it, in order,
    /// each a batch of its own, with how many records it folded.
    fn merged_in_batches(batches: &[Vec<(Value, SourcePosition)>]) -> (Value, u64, Option<String>) {
        let numbers = WriteBatches::new();
        let mut slot: Option<LatestSlot> = None;
        for records in batches {
            let batch = numbers.next();
            for (data, position) in records {
                let record = incoming(data.clone(), *position, batch);
                match &mut slot {
                    Some(slot) => slot.absorb(record, &SourceReads::Everything),
                    None => slot = Some(LatestSlot::new(record)),
                }
            }
        }
        let (fragment, _) = slot
            .unwrap()
            .into_stored(&SourceReads::Everything)
            .into_fragment(MappingId::new(1), &SourceReads::Everything);
        (fragment.data, fragment.records.get(), fragment.conflict.map(|path| path.to_string()))
    }

    #[test]
    fn a_field_two_inputs_disagree_on_keeps_the_first_input_s_value_whichever_input_s_batch_arrives_first() {
        let north = vec![(json!({"name": "Main", "capacity": 40}), of_input(0, 0))];
        let south = vec![(json!({"name": "Main", "capacity": 42}), of_input(1, 0))];
        for batches in [[north.clone(), south.clone()], [south, north]] {
            assert_eq!(
                merged_in_batches(&batches),
                (json!({"name": "Main", "capacity": 40}), 2, Some("capacity".to_string()))
            );
        }
    }

    #[test]
    fn three_inputs_join_in_the_order_of_the_manifest_in_every_order_their_batches_arrive_in() {
        let inputs = [
            vec![(json!({"name": "Main", "capacity": 40}), of_input(0, 0))],
            vec![(json!({"capacity": 42, "zone": "B"}), of_input(1, 0))],
            vec![(json!({"capacity": 44, "zone": "C", "level": 3}), of_input(2, 0))],
        ];
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let batches = order.map(|input| inputs[input].clone());
            assert_eq!(
                merged_in_batches(&batches),
                (
                    json!({"name": "Main", "capacity": 40, "zone": "B", "level": 3}),
                    3,
                    Some("capacity".to_string())
                ),
                "{order:?}"
            );
        }
    }

    #[test]
    fn each_input_merges_its_own_records_in_its_order_while_the_batches_of_two_inputs_interleave() {
        let (data, records, conflict) = merged_in_batches(&[
            vec![
                (json!({"capacity": 51, "zone": "S"}), of_input(1, 1)),
                (json!({"capacity": 50}), of_input(1, 0)),
            ],
            vec![(json!({"capacity": 41}), of_input(0, 1)), (json!({"name": "Main"}), of_input(0, 0))],
            vec![
                (json!({"capacity": 52, "level": 2}), of_input(1, 2)),
                (json!({"capacity": 40, "name": "Renamed"}), of_input(0, 2)),
            ],
        ]);

        assert_eq!(
            (data, records, conflict),
            (
                json!({"name": "Main", "capacity": 41, "zone": "S", "level": 2}),
                6,
                Some("capacity".to_string())
            )
        );
    }

    #[test]
    fn records_of_two_inputs_in_one_batch_merge_in_the_order_of_the_manifest_whatever_order_they_arrive_in() {
        let batch = [
            (json!({"capacity": 42, "zone": "B"}), of_input(1, 0)),
            (json!({"capacity": 40}), of_input(0, 3)),
            (json!({"capacity": 41, "name": "Main"}), of_input(0, 7)),
        ];
        for order in [[0, 1, 2], [1, 0, 2], [2, 1, 0], [1, 2, 0]] {
            let arrivals = order.map(|index| batch[index].clone()).to_vec();
            assert_eq!(
                merged_in_batches(&[arrivals]),
                (json!({"capacity": 40, "name": "Main", "zone": "B"}), 3, Some("capacity".to_string())),
                "{order:?}"
            );
        }
    }

    #[test]
    fn an_earlier_input_reaching_a_slot_read_back_from_the_store_still_comes_first() {
        let batches = WriteBatches::new();
        let mut slot = LatestSlot::new(incoming(json!({"capacity": 42, "zone": "B"}), of_input(1, 0), batches.next()));
        slot.absorb(incoming(json!({"capacity": 43}), of_input(1, 1), batches.next()), &SourceReads::Everything);
        let stored = slot.into_stored(&SourceReads::Everything);

        let later = batches.next();
        let mut read_back = LatestSlot::from_stored(stored, later);
        read_back.absorb(
            incoming(json!({"capacity": 40, "name": "Main"}), of_input(0, 0), later),
            &SourceReads::Everything,
        );
        let (fragment, _) = read_back
            .into_stored(&SourceReads::Everything)
            .into_fragment(MappingId::new(1), &SourceReads::Everything);

        assert_eq!(
            (fragment.data, fragment.records.get(), fragment.conflict),
            (json!({"capacity": 40, "name": "Main", "zone": "B"}), 3, Some(FieldPath::new(&["capacity"])))
        );
    }

    #[test]
    fn a_slot_read_back_without_its_inputs_takes_an_input_s_merge_back_before_a_further_record_of_it() {
        let batches = WriteBatches::new();
        let mut north = InputMerge::nothing(0);
        north.absorb(InputMerge::record(0, json!({"name": "Main", "capacity": 40}), None), &SourceReads::Everything);
        let mut south = InputMerge::nothing(1);
        south.absorb(InputMerge::record(1, json!({"capacity": 42}), None), &SourceReads::Everything);
        let stored = StoredSlot::Merged {
            records: NonZeroU64::new(2).unwrap(),
            inputs: SmallVec::new(),
        };

        let later = batches.next();
        let mut slot = LatestSlot::from_stored(stored, later);
        slot.restore_input(south, later);
        slot.absorb(incoming(json!({"capacity": 43, "zone": "B"}), of_input(1, 5), later), &SourceReads::Everything);
        slot.restore_input(north, later);
        let (fragment, _) = slot
            .into_stored(&SourceReads::Everything)
            .into_fragment(MappingId::new(1), &SourceReads::Everything);

        assert_eq!(
            (fragment.data, fragment.records.get(), fragment.conflict),
            (json!({"name": "Main", "capacity": 40, "zone": "B"}), 3, Some(FieldPath::new(&["capacity"])))
        );
    }

    #[test]
    fn keeping_each_input_s_records_apart_costs_a_static_slot_one_word_at_most() {
        // A static mapping's records as one merge, without their inputs: the merged record, its
        // earliest conflict and its expiry, the batch held apart, and its records. The input's place
        // fills the merge's padding, so the list of inputs costs only its length, the word smallvec's
        // `union` layout saves on every `SmallVec`, the held records' among them.
        type OneMerge = ((Value, Option<FieldPath>, MappingExpiry), WriteBatch, SmallVec<[PendingRecord; 1]>);
        assert!(mem::size_of::<MergedRecords>() <= mem::size_of::<OneMerge>() + mem::size_of::<usize>());
    }
}

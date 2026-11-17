use crate::{
    entity_store::{
        mapping_expiry::MappingExpiry,
        merge::deep_merge,
        record_timestamp::RecordTimestamp,
        stored_slot::StoredSlot,
        supersession::{SlotRanking, Supersession},
        write_batch::WriteBatch,
    },
    field_path::FieldPath,
};
use cassiopeia_ir::source_position::SourcePosition;
use cassiopeia_mapping::source_reads::SourceReads;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
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
    /// What the record-level `observedAt` template yielded, for ranking it against the retained record.
    pub(crate) timestamp: RecordTimestamp,
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
    /// A temporal mapping keeps the record with the latest `observedAt`.
    Latest(LatestRecord),
    /// A static mapping merges its records.
    Merged(MergedRecords),
}

/// A temporal mapping's retained record.
///
/// Records are ranked by their timestamp and then their position in the source, so the record
/// retained is the same whichever order they arrive in.
#[derive(Debug)]
struct LatestRecord {
    /// The retained record.
    data: Value,
    /// How the slot ranks the mapping's records.
    ranking: SlotRanking,
    /// The entity `expiresAt` of the retained record.
    expires_at: MappingExpiry,
}

/// A static mapping's records, merged in the order of the source.
///
/// Which value a field the records disagree on keeps depends on the order they are merged in, and
/// the records of one batch reach the store in whatever order the resolver's threads finish them.
/// The records of the latest batch are therefore held apart and merged in source order once a write
/// of a later batch, or assembly, shows that none of the batch's records is still to come. Every
/// earlier batch came before the batch held apart, so merging batch by batch keeps the source order
/// across batches too.
#[derive(Debug)]
struct MergedRecords {
    /// The records of every batch before `batch`, merged.
    merged: Merge,
    /// The batch `pending` holds records of.
    batch: WriteBatch,
    /// The records of `batch`, not merged yet. One record in the common case of an entity with one
    /// record in a batch, which is held without a heap allocation.
    pending: SmallVec<[PendingRecord; 1]>,
}

/// Records merged into one.
#[derive(Debug)]
struct Merge {
    /// The merged record.
    data: Value,
    /// The earliest field the mapping reads on which the merged records disagreed.
    conflict: Option<FieldPath>,
    /// The entity `expiresAt` the merged records contribute.
    expires_at: MappingExpiry,
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
    /// A slot holding the mapping's first record for the id, kept as a `temporal` mapping keeps its
    /// records, or as a static one does.
    pub(crate) fn new(incoming: IncomingRecord, temporal: bool) -> LatestSlot {
        let kept = if temporal {
            Kept::Latest(LatestRecord {
                data: incoming.data,
                ranking: SlotRanking::first(incoming.timestamp, incoming.position),
                expires_at: MappingExpiry::first(incoming.expires_at),
            })
        } else {
            Kept::Merged(MergedRecords {
                merged: Merge::nothing(),
                batch: incoming.batch,
                pending: smallvec![PendingRecord::from(incoming)],
            })
        };
        LatestSlot {
            records: NonZeroU64::MIN,
            kept,
        }
    }

    /// Folds a further record of the mapping into the slot and counts it. A static mapping's
    /// disagreements count only beneath a key in `reads`.
    pub(crate) fn absorb(&mut self, incoming: IncomingRecord, reads: &SourceReads) {
        self.records = self.records.saturating_add(1);
        match &mut self.kept {
            // A temporal mapping's records are observations of one id: keep the latest, expiry and all.
            Kept::Latest(latest) => match latest.ranking.rank(incoming.timestamp, incoming.position) {
                Supersession::Supersedes => {
                    latest.data = incoming.data;
                    latest.expires_at.supersede(incoming.expires_at);
                }
                Supersession::Stale => {}
            },
            Kept::Merged(merged) => {
                // The resolver writes every record of a batch before any of the next, so a record of a
                // later batch means the held records are all there.
                if incoming.batch != merged.batch {
                    merged.seal(reads);
                    merged.batch = incoming.batch;
                }
                merged.pending.push(PendingRecord::from(incoming));
            }
        }
    }

    /// The slot `stored` holds, in the batch `batch` reads it back for.
    pub(crate) fn from_stored(stored: StoredSlot, batch: WriteBatch) -> LatestSlot {
        match stored {
            StoredSlot::Latest {
                records,
                data,
                ranking,
                expires_at,
            } => LatestSlot {
                records,
                kept: Kept::Latest(LatestRecord { data, ranking, expires_at }),
            },
            StoredSlot::Merged {
                records,
                data,
                conflict,
                expires_at,
            } => LatestSlot {
                records,
                kept: Kept::Merged(MergedRecords {
                    merged: Merge { data, conflict, expires_at },
                    batch,
                    pending: SmallVec::new(),
                }),
            },
        }
    }

    /// The slot as it is stored once every record of its batch has reached it, merging the records it
    /// holds apart with `reads`.
    pub(crate) fn into_stored(self, reads: &SourceReads) -> StoredSlot {
        let LatestSlot { records, kept } = self;
        match kept {
            Kept::Latest(LatestRecord { data, ranking, expires_at }) => StoredSlot::Latest {
                records,
                data,
                ranking,
                expires_at,
            },
            Kept::Merged(mut merged) => {
                merged.seal(reads);
                let Merge { data, conflict, expires_at } = merged.merged;
                StoredSlot::Merged {
                    records,
                    data,
                    conflict,
                    expires_at,
                }
            }
        }
    }
}

impl MergedRecords {
    /// Merges the held records into the merged ones in source order, counting a disagreement only
    /// beneath a key in `reads`.
    fn seal(&mut self, reads: &SourceReads) {
        // Stable, so two records sharing a position, which only one record expanding into the same
        // entity twice gives, keep the order they arrived in.
        if !self.pending.is_sorted_by_key(|record| record.position) {
            self.pending.sort_by_key(|record| record.position);
        }
        for record in self.pending.drain(..) {
            self.merged.absorb(record, reads);
        }
    }
}

impl Merge {
    /// What no record has been merged into yet.
    fn nothing() -> Merge {
        Merge {
            data: Value::Object(Map::new()),
            conflict: None,
            expires_at: MappingExpiry::first(None),
        }
    }

    /// Merges `record` into the records merged so far, which all came before it in the source.
    fn absorb(&mut self, record: PendingRecord, reads: &SourceReads) {
        if self.data.as_object().is_some_and(Map::is_empty) {
            // Merging into nothing yields the record itself, so it is moved in rather than copied
            // field by field.
            self.data = record.data;
        } else {
            let conflict = deep_merge(&mut self.data, &record.data, reads);
            self.conflict = FieldPath::earliest(self.conflict.take(), conflict);
        }
        self.expires_at.refine(record.expires_at);
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
            latest_slot::{IncomingRecord, LatestSlot},
            mapping_expiry::MappingExpiry,
            record_timestamp::RecordTimestamp,
            write_batch::{WriteBatch, WriteBatches},
        },
        mapping_id::MappingId,
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use cassiopeia_mapping::{observed_at::ObservedAt, source_reads::SourceReads};
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    fn instant(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn incoming(data: Value, position: SourcePosition, batch: WriteBatch) -> IncomingRecord {
        IncomingRecord {
            data,
            position,
            batch,
            timestamp: RecordTimestamp::Absent,
            expires_at: None,
        }
    }

    /// The fragment a static slot fed `records`, in order, assembles into.
    fn merged(records: Vec<IncomingRecord>) -> (Value, Option<String>, MappingExpiry) {
        let mut records = records.into_iter();
        let mut slot = LatestSlot::new(records.next().unwrap(), false);
        for record in records {
            slot.absorb(record, &SourceReads::Everything);
        }
        let (fragment, expiry) = slot.into_stored(&SourceReads::Everything).into_fragment(MappingId::new(1));
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
        let mut slot = LatestSlot::new(incoming(json!({"capacity": 40, "name": "Main"}), at(0), batches.next()), false);
        slot.absorb(incoming(json!({"capacity": 41}), at(1), batches.next()), &SourceReads::Everything);
        let stored = slot.into_stored(&SourceReads::Everything);

        let later = batches.next();
        let mut read_back = LatestSlot::from_stored(stored, later);
        read_back.absorb(incoming(json!({"capacity": 42, "zone": "C"}), at(3), later), &SourceReads::Everything);
        read_back.absorb(incoming(json!({"capacity": 43}), at(2), later), &SourceReads::Everything);
        let (fragment, _) = read_back.into_stored(&SourceReads::Everything).into_fragment(MappingId::new(1));

        assert_eq!(
            (fragment.data, fragment.records.get(), fragment.conflict.map(|path| path.to_string())),
            (json!({"capacity": 40, "name": "Main", "zone": "C"}), 4, Some("capacity".to_string()))
        );
    }

    #[test]
    fn a_temporal_slot_keeps_the_latest_record_whichever_order_records_arrive_in() {
        let batch = WriteBatches::new().next();
        let observed = |data: Value, position, time: &str| IncomingRecord {
            timestamp: RecordTimestamp::Declared(ObservedAt::new(time)),
            ..incoming(data, position, batch)
        };
        let mut slot = LatestSlot::new(observed(json!({"t": 3}), at(3), "2026-03-01T11:00:00Z"), true);
        slot.absorb(observed(json!({"t": 1}), at(1), "2026-03-01T11:00:00Z"), &SourceReads::Everything);
        slot.absorb(observed(json!({"t": 0}), at(0), "2026-03-01T10:00:00Z"), &SourceReads::Everything);

        let (fragment, _) = slot.into_stored(&SourceReads::Everything).into_fragment(MappingId::new(1));
        assert_eq!((fragment.data, fragment.records.get()), (json!({"t": 3}), 3));
    }
}

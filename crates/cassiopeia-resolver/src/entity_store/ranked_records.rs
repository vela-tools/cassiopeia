use crate::{
    entity_store::{
        discarded_timestamps::DiscardedTimestamps,
        mapping_expiry::MappingExpiry,
        observations::Observations,
        record_failures::RecordFailures,
        store::AssembledFragment,
        supersession::{RecordOrdinal, SlotRanking, Supersession},
    },
    mapping_id::MappingId,
};
use cassiopeia_ir::{
    observed_records::{ObservedRecords, RecordChoice},
    source_position::SourcePosition,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{iter, mem, num::NonZeroU64};

/// One record of a temporal mapping on its way into a current-state slot.
pub(crate) struct ObservedRecord {
    /// The source record.
    pub(crate) data: Value,
    /// Where the record stands in the source.
    pub(crate) position: SourcePosition,
    /// Which of the mapping's records for the id it is.
    pub(crate) record: RecordOrdinal,
    /// What the record ranks by.
    pub(crate) observations: Observations,
    /// The entity `expiresAt` the record declares.
    pub(crate) expires_at: Option<DateTime<Utc>>,
}

/// A temporal mapping's records for one id in a current-state store: the record observed last, and
/// the record each slot of the mapping keeps.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.5.3 resolves an attribute's conflicting instances per
/// attribute, by the most recent `observedAt`. Each slot ranks the records by the instant its
/// template names and keeps the latest, so every attribute it ranks comes from the record where it
/// was observed last; everything no slot ranks, and the entity `expiresAt`, come from the record
/// observed last over every template. A record no ranking holds is released at once, so the records
/// kept never outnumber the slots by more than one: one record for a mapping without slots, as for
/// every mapping whose attributes share one template. A released record's losses are counted as it
/// is released, while those of a record kept are left for the extractor, which meets them, so each
/// is reported once whichever record wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RankedRecords {
    /// Which record was observed last.
    newest: SlotRanking,
    /// The record observed last.
    newest_record: RetainedRecord,
    /// Which record each slot keeps, slot by slot. Empty for a mapping without slots.
    slots: Vec<SlotRanking>,
    /// The records some slot keeps besides the newest one, each once.
    observed: Vec<RetainedRecord>,
    /// The entity `expiresAt` of the record observed last.
    expires_at: MappingExpiry,
    /// The losses of the records released.
    discarded: DiscardedTimestamps,
}

/// A record some ranking holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RetainedRecord {
    /// Which of the mapping's records it is.
    record: RecordOrdinal,
    /// The source record.
    data: Value,
    /// The losses its templates met, counted should the record be released.
    failures: RecordFailures,
}

impl RankedRecords {
    /// The records of a mapping that has just taken its first record for an id.
    pub(crate) fn first(incoming: ObservedRecord) -> RankedRecords {
        let ObservedRecord {
            data,
            position,
            record,
            observations,
            expires_at,
        } = incoming;
        let (latest, slots, failures) = observations.into_parts();
        // A first record holds every ranking, so it opens each slot its mapping has.
        let slots = slots.into_iter().map(|instant| SlotRanking::first(instant, position, record)).collect();
        RankedRecords {
            newest: SlotRanking::first(latest, position, record),
            newest_record: RetainedRecord { record, data, failures },
            slots,
            observed: Vec::new(),
            expires_at: MappingExpiry::first(expires_at),
            discarded: DiscardedTimestamps::default(),
        }
    }

    /// Ranks a further record of the mapping against those kept, keeping it where it is later and
    /// releasing whatever no ranking holds any longer.
    pub(crate) fn absorb(&mut self, incoming: ObservedRecord) {
        let ObservedRecord {
            data,
            position,
            record,
            observations,
            expires_at,
        } = incoming;
        let (latest, slots, failures) = observations.into_parts();
        let mut held = false;
        // Every record of a mapping carries one instant per slot; one carrying fewer ranks as
        // observed at no instant in the slots it lacks.
        for (slot, instant) in self.slots.iter_mut().zip(slots.into_iter().chain(iter::repeat(None))) {
            held |= slot.rank(instant, position, record) == Supersession::Supersedes;
        }
        let retained = RetainedRecord { record, data, failures };
        match self.newest.rank(latest, position, record) {
            Supersession::Supersedes => {
                self.expires_at.supersede(expires_at);
                let previous = mem::replace(&mut self.newest_record, retained);
                // Checked before the record is set aside, so a mapping without slots, the common
                // case, never grows the list of records kept besides the newest one.
                if self.holds(previous.record) {
                    self.observed.push(previous);
                } else {
                    self.discarded.discard(previous.failures);
                }
            }
            Supersession::Stale if held => self.observed.push(retained),
            Supersession::Stale => self.discarded.discard(retained.failures),
        }
        if held {
            self.release_unheld();
        }
    }

    /// Whether some slot holds the record `record`.
    fn holds(&self, record: RecordOrdinal) -> bool {
        self.slots.iter().any(|slot| slot.record() == record)
    }

    /// Releases every record besides the newest that no slot holds any longer, counting its losses.
    fn release_unheld(&mut self) {
        let RankedRecords {
            slots, observed, discarded, ..
        } = self;
        observed.retain_mut(|retained| {
            let held = slots.iter().any(|slot| slot.record() == retained.record);
            if !held {
                discarded.discard(mem::take(&mut retained.failures));
            }
            held
        });
    }

    /// The records as the fragment of the mapping `mapping_id`, which folded `records` records into
    /// them, with the expiry they contribute.
    pub(crate) fn into_fragment(self, mapping_id: MappingId, records: NonZeroU64) -> (AssembledFragment, MappingExpiry) {
        let RankedRecords {
            newest_record,
            slots,
            observed,
            expires_at,
            discarded,
            ..
        } = self;
        let observed = if observed.is_empty() {
            None
        } else {
            let by_slot = slots
                .iter()
                .map(|slot| {
                    // A slot holding no record among the others holds the newest one.
                    observed
                        .iter()
                        .position(|retained| retained.record == slot.record())
                        .map_or(RecordChoice::Newest, RecordChoice::Observed)
                })
                .collect();
            Some(Box::new(ObservedRecords::new(
                observed.into_iter().map(|retained| retained.data).collect(),
                by_slot,
            )))
        };
        let fragment = AssembledFragment {
            mapping_id,
            data: newest_record.data,
            observed,
            records,
            conflict: None,
            discarded,
        };
        (fragment, expires_at)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            discarded_timestamps::UnreadableDiscards,
            mapping_expiry::MappingExpiry,
            observations::Observations,
            ranked_records::{ObservedRecord, RankedRecords},
            record_timestamp::RecordTimestamp,
            store::AssembledFragment,
            supersession::RecordOrdinal,
        },
        mapping_id::MappingId,
        test_order::EVERY_ORDER_OF_THREE,
    };
    use cassiopeia_ir::{
        observed_records::{ObservedRecords, RecordChoice},
        source_position::SourcePosition,
    };
    use cassiopeia_mapping::{observed_at::ObservedAt, observed_template::ObservedTemplateIndex};
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::num::NonZeroU64;

    /// One record of a mapping: what it holds, where it stands in the source, what each of its
    /// templates rendered, and the entity expiry it declares.
    #[derive(Clone)]
    struct Record {
        data: Value,
        position: usize,
        rendered: Vec<Value>,
        expires_at: Option<DateTime<Utc>>,
    }

    /// The record at `position` holding `data` whose templates rendered `rendered`.
    fn record(data: Value, position: usize, rendered: &[Value]) -> Record {
        Record {
            data,
            position,
            rendered: rendered.to_vec(),
            expires_at: None,
        }
    }

    /// The record `record` as the `count`-th to arrive, ranked in `slots`.
    fn arriving(record: &Record, count: u64, slots: &[ObservedTemplateIndex]) -> ObservedRecord {
        ObservedRecord {
            data: record.data.clone(),
            position: SourcePosition::first_of(0).advanced(record.position),
            record: RecordOrdinal::new(NonZeroU64::new(count).unwrap()),
            observations: Observations::of(record.rendered.iter().cloned().map(RecordTimestamp::read), slots),
            expires_at: record.expires_at,
        }
    }

    /// The fragment and expiry `records`, arriving in `order`, fold into, ranked in `slots`.
    fn folded(records: &[Record], order: &[usize], slots: &[ObservedTemplateIndex]) -> (AssembledFragment, MappingExpiry) {
        let mut arrivals = order.iter().zip(1..).map(|(index, count)| arriving(&records[*index], count, slots));
        let mut ranked = RankedRecords::first(arrivals.next().unwrap());
        for incoming in arrivals {
            ranked.absorb(incoming);
        }
        let count = u64::try_from(order.len()).unwrap();
        ranked.into_fragment(MappingId::new(1), NonZeroU64::new(count).unwrap())
    }

    /// One slot per template of a mapping declaring two.
    const TWO_SLOTS: [ObservedTemplateIndex; 2] = [ObservedTemplateIndex::new(0), ObservedTemplateIndex::new(1)];

    fn r1() -> Record {
        record(json!({"record": 1}), 0, &[json!("2026-03-01T11:00:00Z"), json!("2026-03-01T09:00:00Z")])
    }

    fn r2() -> Record {
        record(json!({"record": 2}), 1, &[json!("2026-03-01T10:00:00Z"), json!("2026-03-01T12:00:00Z")])
    }

    /// The records a fragment holds besides its newest one when the first slot keeps `kept_first`
    /// and the second slot the newest record.
    fn first_slot_apart(kept_first: Value) -> ObservedRecords {
        ObservedRecords::new(vec![kept_first], vec![RecordChoice::Observed(0), RecordChoice::Newest])
    }

    #[test]
    fn each_slot_keeps_the_record_observed_last_in_it_and_the_newest_is_the_latest_over_all_in_either_order() {
        let records = [r1(), r2()];
        for order in [[0, 1], [1, 0]] {
            let (fragment, _) = folded(&records, &order, &TWO_SLOTS);

            assert_eq!(fragment.data, json!({"record": 2}), "{order:?}");
            assert_eq!(
                fragment.observed.map(|observed| *observed),
                Some(first_slot_apart(json!({"record": 1}))),
                "{order:?}"
            );
        }
    }

    #[test]
    fn what_no_slot_ranks_comes_from_the_record_with_the_latest_observation_in_either_order() {
        let records = [
            record(json!({"record": 1}), 0, &[json!("2026-03-01T12:00:00Z"), json!("2026-03-01T09:00:00Z")]),
            record(json!({"record": 2}), 1, &[json!("2026-03-01T10:00:00Z"), json!("2026-03-01T11:00:00Z")]),
        ];
        for order in [[0, 1], [1, 0]] {
            let (fragment, _) = folded(&records, &order, &TWO_SLOTS);

            assert_eq!(fragment.data, json!({"record": 1}), "{order:?}");
            assert_eq!(
                fragment.observed.map(|observed| *observed),
                Some(ObservedRecords::new(
                    vec![json!({"record": 2})],
                    vec![RecordChoice::Newest, RecordChoice::Observed(0)]
                )),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_record_losing_every_ranking_is_released_whatever_the_order() {
        let r3 = record(json!({"record": 3}), 2, &[json!("2026-03-01T08:00:00Z"), json!("2026-03-01T08:00:00Z")]);
        let records = [r1(), r2(), r3];
        for order in EVERY_ORDER_OF_THREE {
            let (fragment, _) = folded(&records, &order, &TWO_SLOTS);

            assert_eq!(fragment.data, json!({"record": 2}), "{order:?}");
            assert_eq!(
                fragment.observed.map(|observed| *observed),
                Some(first_slot_apart(json!({"record": 1}))),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_shared_template_keeps_the_newer_record_whole_even_without_a_value() {
        let records = [
            record(json!({"t": 20, "time": "10:00"}), 0, &[json!("2026-03-01T10:00:00Z")]),
            record(json!({"time": "11:00"}), 1, &[json!("2026-03-01T11:00:00Z")]),
        ];
        for order in [[0, 1], [1, 0]] {
            let (fragment, _) = folded(&records, &order, &[]);

            assert_eq!((fragment.data, fragment.observed), (json!({"time": "11:00"}), None), "{order:?}");
        }
    }

    #[test]
    fn the_expiry_follows_the_newest_record_in_either_order() {
        let mut expiring = r1();
        expiring.expires_at = Utc.timestamp_opt(1_800_000_000, 0).single();
        let mut unexpiring = r2();
        unexpiring.expires_at = None;
        let records = [expiring, unexpiring];
        for order in [[0, 1], [1, 0]] {
            let (_, expiry) = folded(&records, &order, &TWO_SLOTS);

            assert_eq!(expiry, MappingExpiry::first(None), "{order:?}");
        }
    }

    #[test]
    fn a_record_kept_for_one_slot_is_not_counted_for_a_template_that_failed_on_it() {
        let failing_second = record(json!({"record": 1}), 0, &[json!("2026-03-01T11:00:00Z"), json!("soon")]);
        let records = [failing_second, r2()];
        for order in [[0, 1], [1, 0]] {
            let (fragment, _) = folded(&records, &order, &TWO_SLOTS);

            assert!(fragment.discarded.is_empty(), "{order:?}");
            assert_eq!(
                fragment.observed.map(|observed| *observed),
                Some(first_slot_apart(json!({"record": 1}))),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_record_losing_every_ranking_is_counted_once_for_its_losses() {
        let losing = record(json!({"record": 0}), 2, &[json!("soon"), json!("soon")]);
        let records = [r1(), r2(), losing];
        for order in EVERY_ORDER_OF_THREE {
            let (fragment, _) = folded(&records, &order, &TWO_SLOTS);

            assert_eq!(
                fragment.discarded.unreadable(),
                [0, 1].map(|template| UnreadableDiscards {
                    template: ObservedTemplateIndex::new(template),
                    example: ObservedAt::new("soon"),
                    records: NonZeroU64::MIN,
                }),
                "{order:?}"
            );
            assert_eq!(fragment.discarded.unresolvable(), 0, "{order:?}");
        }
    }

    #[test]
    fn a_record_whose_only_template_failed_is_counted_once_as_it_loses() {
        let records = [
            record(json!({"record": 1}), 0, &[json!("2026-03-01T11:00:00Z")]),
            record(json!({"record": 2}), 1, &[json!(null)]),
        ];
        let mut ranked = RankedRecords::first(arriving(&records[0], 1, &[]));
        ranked.absorb(ObservedRecord {
            observations: Observations::of([RecordTimestamp::Unresolvable], &[]),
            ..arriving(&records[1], 2, &[])
        });

        let (fragment, _) = ranked.into_fragment(MappingId::new(1), NonZeroU64::new(2).unwrap());
        assert_eq!((fragment.data, fragment.discarded.unresolvable()), (json!({"record": 1}), 1));
    }

    #[test]
    fn ranked_records_read_back_from_the_store_take_further_records_by_the_same_rules() {
        let r3 = record(json!({"record": 3}), 2, &[json!("2026-03-01T08:00:00Z"), json!("2026-03-01T08:00:00Z")]);
        let mut ranked = RankedRecords::first(arriving(&r1(), 1, &TWO_SLOTS));
        let encoded = rmp_serde::to_vec(&ranked).unwrap();
        let mut read_back: RankedRecords = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(read_back, ranked);

        for slot in [&mut ranked, &mut read_back] {
            slot.absorb(arriving(&r3, 2, &TWO_SLOTS));
            slot.absorb(arriving(&r2(), 3, &TWO_SLOTS));
        }
        assert_eq!(read_back, ranked);
        let (fragment, _) = read_back.into_fragment(MappingId::new(1), NonZeroU64::new(3).unwrap());
        assert_eq!(fragment.observed.map(|observed| *observed), Some(first_slot_apart(json!({"record": 1}))));
    }
}

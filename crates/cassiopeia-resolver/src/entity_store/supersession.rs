use crate::entity_store::observation_instant::ObservationInstant;
use cassiopeia_ir::source_position::SourcePosition;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// Whether an incoming temporal record takes a ranking from the record holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supersession {
    /// The incoming record is later than the one holding the ranking and replaces it.
    Supersedes,
    /// The incoming record is earlier than the one holding the ranking and does not take it.
    Stale,
}

/// Which of the records a mapping resolved to one id a ranking names: the count of the mapping's
/// records for the id when it arrived.
///
/// A record's position cannot tell two records apart when one record expands into the same entity
/// twice, so a ranking names the record it holds by when it arrived instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordOrdinal(NonZeroU64);

impl RecordOrdinal {
    /// The record that arrived as the `count`-th of its mapping for an id.
    #[must_use]
    pub const fn new(count: NonZeroU64) -> RecordOrdinal {
        RecordOrdinal(count)
    }
}

/// One ranking of a temporal mapping's records in a current-state store, and the record holding it.
///
/// A ranking is held by the record whose `observedAt` names the latest instant (ETSI GS CIM 009
/// v1.9.1 clause 4.5.5.3 keeps "the one with the most recent `observedAt` `DateTime`"). Instants are
/// compared, not the text naming them, so two spellings of one instant rank as one. A record without
/// an instant ranks below every record with one (`None < Some`), whether it carries no `observedAt`,
/// text that reads as no instant, or a template that failed, and a tie keeps the record later in
/// the source, by its [`SourcePosition`], whichever order the records reach the store in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRanking {
    /// The instant of the record holding the ranking.
    instant: Option<ObservationInstant>,
    /// Where the record holding the ranking stands in the source, which settles a tie between two
    /// records naming one instant.
    position: SourcePosition,
    /// The record holding the ranking.
    record: RecordOrdinal,
}

impl SlotRanking {
    /// The ranking held by the first record, `record`, observed at `instant` and standing at
    /// `position`.
    #[must_use]
    pub const fn first(instant: Option<ObservationInstant>, position: SourcePosition, record: RecordOrdinal) -> SlotRanking {
        SlotRanking { instant, position, record }
    }

    /// Ranks an incoming record, `record`, observed at `instant` and standing at `position`, against
    /// the one holding the ranking, which the later of the two then holds.
    pub fn rank(&mut self, instant: Option<ObservationInstant>, position: SourcePosition, record: RecordOrdinal) -> Supersession {
        if (instant, position) >= (self.instant, self.position) {
            *self = SlotRanking { instant, position, record };
            Supersession::Supersedes
        } else {
            Supersession::Stale
        }
    }

    /// The record holding the ranking.
    #[must_use]
    pub const fn record(&self) -> RecordOrdinal {
        self.record
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        observation_instant::ObservationInstant,
        record_timestamp::RecordTimestamp,
        supersession::{RecordOrdinal, SlotRanking, Supersession},
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::num::NonZeroU64;

    /// The instant a record whose `observedAt` renders as `rendered` ranks by.
    fn observed(rendered: Value) -> Option<ObservationInstant> {
        RecordTimestamp::read(rendered).instant()
    }

    fn declared(text: &str) -> Option<ObservationInstant> {
        observed(json!(text))
    }

    /// The position of record `record` of the first input.
    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    fn ordinal(count: u64) -> RecordOrdinal {
        RecordOrdinal::new(NonZeroU64::new(count).unwrap())
    }

    /// Which of two records a ranking keeps, the earlier one in the source observed at `first` and
    /// the later at `second`, when they arrive in source order and when they arrive the other way
    /// round.
    fn kept_in_both_orders(first: Option<ObservationInstant>, second: Option<ObservationInstant>) -> [SourcePosition; 2] {
        let mut in_order = SlotRanking::first(first, at(0), ordinal(1));
        in_order.rank(second, at(1), ordinal(2));
        let mut reversed = SlotRanking::first(second, at(1), ordinal(1));
        reversed.rank(first, at(0), ordinal(2));
        [in_order.position, reversed.position]
    }

    #[test]
    fn a_later_timestamp_supersedes_an_earlier_one() {
        let mut ranking = SlotRanking::first(declared("2026-04-03T22:00:20Z"), at(0), ordinal(1));
        assert_eq!(ranking.rank(declared("2026-04-03T22:05:20Z"), at(1), ordinal(2)), Supersession::Supersedes);
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z"), at(2), ordinal(3)), Supersession::Stale);
        assert_eq!(ranking.record(), ordinal(2));
    }

    #[test]
    fn an_earlier_instant_spelled_with_an_offset_loses_to_a_later_one_in_utc_in_either_order() {
        assert_eq!(
            kept_in_both_orders(declared("2026-03-01T11:00:00Z"), declared("2026-03-01T12:00:00+02:00")),
            [at(0), at(0)]
        );
    }

    #[test]
    fn one_instant_in_two_spellings_keeps_the_record_later_in_the_source_in_either_order() {
        assert_eq!(
            kept_in_both_orders(declared("2026-03-01T11:00:00Z"), declared("2026-03-01T12:00:00+01:00")),
            [at(1), at(1)]
        );
    }

    #[test]
    fn epochs_rank_by_the_instant_they_name_not_by_their_text() {
        assert_eq!(
            kept_in_both_orders(observed(json!(999_999_999)), observed(json!(1_000_000_000))),
            [at(1), at(1)]
        );
        assert_eq!(kept_in_both_orders(declared("999999999"), declared("1000000000")), [at(1), at(1)]);
    }

    #[test]
    fn a_readable_timestamp_beats_unreadable_text_in_either_order() {
        assert_eq!(kept_in_both_orders(declared("2026-03-01T11:00:00Z"), declared("soon")), [at(0), at(0)]);
        assert_eq!(kept_in_both_orders(declared("soon"), declared("2026-03-01T11:00:00Z")), [at(1), at(1)]);
    }

    #[test]
    fn an_absent_timestamp_ranks_below_a_readable_one_in_either_order() {
        assert_eq!(kept_in_both_orders(declared("2026-03-01T11:00:00Z"), None), [at(0), at(0)]);
        assert_eq!(kept_in_both_orders(None, declared("2026-03-01T11:00:00Z")), [at(1), at(1)]);
    }

    #[test]
    fn records_without_an_instant_keep_the_one_later_in_the_source_in_either_order() {
        assert_eq!(kept_in_both_orders(declared("soon"), None), [at(1), at(1)]);
    }

    #[test]
    fn a_tie_keeps_the_record_later_in_the_source_whichever_arrives_first() {
        let time = declared("2026-03-01T11:00:00Z");
        let mut later_first = SlotRanking::first(time, at(7), ordinal(1));
        assert_eq!(later_first.rank(time, at(3), ordinal(2)), Supersession::Stale);
        assert_eq!(later_first.rank(time, at(5), ordinal(3)), Supersession::Stale);

        let mut earlier_first = SlotRanking::first(time, at(3), ordinal(1));
        assert_eq!(earlier_first.rank(time, at(7), ordinal(2)), Supersession::Supersedes);
        assert_eq!(earlier_first.rank(time, at(5), ordinal(3)), Supersession::Stale);
    }

    #[test]
    fn a_later_timestamp_wins_even_from_a_record_earlier_in_the_source() {
        let mut ranking = SlotRanking::first(declared("2026-03-01T11:00:00Z"), at(9), ordinal(1));
        assert_eq!(ranking.rank(declared("2026-03-01T12:00:00Z"), at(1), ordinal(2)), Supersession::Supersedes);
        assert_eq!(ranking.rank(declared("2026-03-01T11:30:00Z"), at(20), ordinal(3)), Supersession::Stale);
    }

    #[test]
    fn a_slot_ranking_survives_the_store_encoding() {
        for ranking in [
            SlotRanking::first(declared("2026-03-01T11:00:00Z"), at(4), ordinal(2)),
            SlotRanking::first(None, at(4), ordinal(1)),
        ] {
            let encoded = rmp_serde::to_vec(&ranking).unwrap();
            assert_eq!(rmp_serde::from_slice::<SlotRanking>(&encoded).unwrap(), ranking);
        }
    }
}

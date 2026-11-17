use crate::entity_store::record_timestamp::RecordTimestamp;
use cassiopeia_ir::source_position::SourcePosition;
use serde::{Deserialize, Serialize};
use std::mem;

/// Whether an incoming temporal record takes a current-state slot from the record it retains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supersession {
    /// The incoming record is later than the retained one and replaces it.
    Supersedes,
    /// The incoming record is earlier than the retained one and is discarded.
    Stale,
}

/// How a current-state slot ranks a temporal mapping's records, and what that ranking discarded.
///
/// A current-state store keeps, per mapping, the record with the greatest record-level `observedAt`.
/// An absent timestamp ranks below any present one (`None < Some`), and a tie keeps the record later
/// in the source, by its [`SourcePosition`], whichever order the records reach the store in.
/// Timestamps are compared as verbatim text, monotonic under string compare for a fixed format
/// (RFC-3339, or fixed-width epoch); this is the record-level latest the store ranks by.
///
/// A record whose `observedAt` template failed ranks as having no timestamp, so another record of
/// the same id usually takes the slot from it. The record that keeps the slot reaches the extractor,
/// which reports the failure itself; a record the slot discards never does, so the slot counts
/// those, and each failure is reported exactly once whichever record wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRanking {
    /// The timestamp of the record the slot retains.
    timestamp: RecordTimestamp,
    /// Where the record the slot retains stands in the source, which settles a tie between two
    /// records naming one timestamp.
    position: SourcePosition,
    /// How many records whose `observedAt` template failed the slot discarded.
    unresolvable_discards: u64,
}

impl SlotRanking {
    /// The ranking of a slot that has just retained its first record, the one at `position`.
    #[must_use]
    pub const fn first(timestamp: RecordTimestamp, position: SourcePosition) -> SlotRanking {
        SlotRanking {
            timestamp,
            position,
            unresolvable_discards: 0,
        }
    }

    /// Ranks an incoming record, the one at `position`, against the retained one, keeping the later
    /// of the two and counting the other among the discards when its template failed.
    pub fn rank(&mut self, incoming: RecordTimestamp, position: SourcePosition) -> Supersession {
        let later = (incoming.ranking_text(), position) >= (self.timestamp.ranking_text(), self.position);
        let (supersession, discarded) = if later {
            self.position = position;
            (Supersession::Supersedes, mem::replace(&mut self.timestamp, incoming))
        } else {
            (Supersession::Stale, incoming)
        };
        if discarded == RecordTimestamp::Unresolvable {
            self.unresolvable_discards = self.unresolvable_discards.saturating_add(1);
        }
        supersession
    }

    /// How many records whose `observedAt` template failed the slot discarded.
    #[must_use]
    pub const fn unresolvable_discards(&self) -> u64 {
        self.unresolvable_discards
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        record_timestamp::RecordTimestamp,
        supersession::{SlotRanking, Supersession},
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use cassiopeia_mapping::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;

    fn declared(text: &str) -> RecordTimestamp {
        RecordTimestamp::Declared(ObservedAt::new(text))
    }

    /// The position of record `record` of the first input.
    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    #[test]
    fn a_later_timestamp_supersedes_an_earlier_one() {
        let mut ranking = SlotRanking::first(declared("2026-04-03T22:00:20Z"), at(0));
        assert_eq!(ranking.rank(declared("2026-04-03T22:05:20Z"), at(1)), Supersession::Supersedes);
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z"), at(2)), Supersession::Stale);
    }

    #[test]
    fn a_present_timestamp_supersedes_an_absent_one_and_a_tie_keeps_the_record_later_in_the_source() {
        let mut ranking = SlotRanking::first(RecordTimestamp::Absent, at(0));
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z"), at(1)), Supersession::Supersedes);
        assert_eq!(ranking.rank(RecordTimestamp::Absent, at(2)), Supersession::Stale);
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z"), at(3)), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 0);
    }

    #[test]
    fn a_failed_record_arriving_after_a_timestamped_one_is_counted_as_discarded() {
        let mut ranking = SlotRanking::first(declared("2026-10-25T08:00:00Z"), at(0));

        assert_eq!(ranking.rank(RecordTimestamp::Unresolvable, at(1)), Supersession::Stale);
        assert_eq!(ranking.unresolvable_discards(), 1);
    }

    #[test]
    fn a_failed_record_displaced_by_a_timestamped_one_is_counted_as_discarded() {
        let mut ranking = SlotRanking::first(RecordTimestamp::Unresolvable, at(0));

        assert_eq!(ranking.rank(declared("2026-10-25T08:00:00Z"), at(1)), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 1);
    }

    #[test]
    fn a_failed_record_that_keeps_the_slot_is_not_counted() {
        // A tie keeps the record later in the source, so the second failed record displaces the first:
        // one discard, and the retained one is left for the extractor to report.
        let mut ranking = SlotRanking::first(RecordTimestamp::Absent, at(0));

        assert_eq!(ranking.rank(RecordTimestamp::Unresolvable, at(1)), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 0);
        assert_eq!(ranking.rank(RecordTimestamp::Unresolvable, at(2)), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 1);
    }

    #[test]
    fn a_tie_keeps_the_record_later_in_the_source_whichever_arrives_first() {
        let time = "2026-03-01T11:00:00Z";
        let mut later_first = SlotRanking::first(declared(time), at(7));
        assert_eq!(later_first.rank(declared(time), at(3)), Supersession::Stale);
        assert_eq!(later_first.rank(declared(time), at(5)), Supersession::Stale);

        let mut earlier_first = SlotRanking::first(declared(time), at(3));
        assert_eq!(earlier_first.rank(declared(time), at(7)), Supersession::Supersedes);
        assert_eq!(earlier_first.rank(declared(time), at(5)), Supersession::Stale);
    }

    #[test]
    fn a_later_timestamp_wins_even_from_a_record_earlier_in_the_source() {
        let mut ranking = SlotRanking::first(declared("2026-03-01T11:00:00Z"), at(9));
        assert_eq!(ranking.rank(declared("2026-03-01T12:00:00Z"), at(1)), Supersession::Supersedes);
        assert_eq!(ranking.rank(declared("2026-03-01T11:30:00Z"), at(20)), Supersession::Stale);
    }

    #[test]
    fn a_slot_ranking_survives_the_store_encoding() {
        let ranking = SlotRanking::first(declared("2026-03-01T11:00:00Z"), at(4));
        let encoded = rmp_serde::to_vec(&ranking).unwrap();
        assert_eq!(rmp_serde::from_slice::<SlotRanking>(&encoded).unwrap(), ranking);
    }
}

use crate::entity_store::record_timestamp::RecordTimestamp;
use serde::{Deserialize, Serialize};
use std::mem;

/// Whether an incoming temporal record takes a current-state slot from the record it retains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supersession {
    /// The incoming record is at least as late as the retained one and replaces it.
    Supersedes,
    /// The incoming record is earlier than the retained one and is discarded.
    Stale,
}

/// How a current-state slot ranks a temporal mapping's records, and what that ranking discarded.
///
/// A current-state store keeps, per mapping, the record with the greatest record-level `observedAt`.
/// An absent timestamp ranks below any present one (`None < Some`), and a tie keeps the later-arriving
/// record. Timestamps are compared as verbatim text, monotonic under string compare for a fixed format
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
    /// How many records whose `observedAt` template failed the slot discarded.
    unresolvable_discards: u64,
}

impl SlotRanking {
    /// The ranking of a slot that has just retained its first record.
    #[must_use]
    pub const fn first(timestamp: RecordTimestamp) -> SlotRanking {
        SlotRanking {
            timestamp,
            unresolvable_discards: 0,
        }
    }

    /// Ranks an incoming record against the retained one, keeping the later of the two and counting
    /// the other among the discards when its template failed.
    pub fn rank(&mut self, incoming: RecordTimestamp) -> Supersession {
        let (supersession, discarded) = if incoming.ranking_text() >= self.timestamp.ranking_text() {
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
    use cassiopeia_mapping::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;

    fn declared(text: &str) -> RecordTimestamp {
        RecordTimestamp::Declared(ObservedAt::new(text))
    }

    #[test]
    fn a_later_timestamp_supersedes_an_earlier_one() {
        let mut ranking = SlotRanking::first(declared("2026-04-03T22:00:20Z"));
        assert_eq!(ranking.rank(declared("2026-04-03T22:05:20Z")), Supersession::Supersedes);
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z")), Supersession::Stale);
    }

    #[test]
    fn a_present_timestamp_supersedes_an_absent_one_and_a_tie_keeps_the_later_arrival() {
        let mut ranking = SlotRanking::first(RecordTimestamp::Absent);
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z")), Supersession::Supersedes);
        assert_eq!(ranking.rank(RecordTimestamp::Absent), Supersession::Stale);
        assert_eq!(ranking.rank(declared("2026-04-03T22:00:20Z")), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 0);
    }

    #[test]
    fn a_failed_record_arriving_after_a_timestamped_one_is_counted_as_discarded() {
        let mut ranking = SlotRanking::first(declared("2026-10-25T08:00:00Z"));

        assert_eq!(ranking.rank(RecordTimestamp::Unresolvable), Supersession::Stale);
        assert_eq!(ranking.unresolvable_discards(), 1);
    }

    #[test]
    fn a_failed_record_displaced_by_a_timestamped_one_is_counted_as_discarded() {
        let mut ranking = SlotRanking::first(RecordTimestamp::Unresolvable);

        assert_eq!(ranking.rank(declared("2026-10-25T08:00:00Z")), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 1);
    }

    #[test]
    fn a_failed_record_that_keeps_the_slot_is_not_counted() {
        // A tie keeps the later arrival, so the second failed record displaces the first: one discard,
        // and the retained one is left for the extractor to report.
        let mut ranking = SlotRanking::first(RecordTimestamp::Absent);

        assert_eq!(ranking.rank(RecordTimestamp::Unresolvable), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 0);
        assert_eq!(ranking.rank(RecordTimestamp::Unresolvable), Supersession::Supersedes);
        assert_eq!(ranking.unresolvable_discards(), 1);
    }
}

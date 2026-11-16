use crate::seen_entities::SeenEntities;
use cassiopeia_sensorthings::reach_back::reach_back;
use chrono::{DateTime, Utc};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How far one stream has been read, a data stream's Observations or a Thing's `HistoricalLocations`:
/// the read position a cursor keeps for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CopyGetters, Getters)]
pub struct Position {
    /// The latest time written for the stream: a reading time, or a `HistoricalLocation`'s `time`.
    #[getset(get_copy = "pub")]
    high_water: DateTime<Utc>,
    /// The entities written for the stream whose time is at or after the high-water mark minus the
    /// lookback.
    #[getset(get = "pub")]
    seen: SeenEntities,
    /// The start of the last cycle that listed the stream.
    #[getset(get_copy = "pub")]
    last_listed: DateTime<Utc>,
    /// The start of the first cycle that no longer listed the stream, or `None` while it is listed.
    #[getset(get_copy = "pub")]
    missing_since: Option<DateTime<Utc>>,
}

impl Position {
    /// The first position of a stream that the cycle that started at `cycle_start` listed, and for
    /// which it wrote `written`: its high-water mark is the latest of them. `None` when the cycle
    /// wrote none, so the stream starts at the initial point again next cycle.
    pub(crate) fn first(written: SeenEntities, cycle_start: DateTime<Utc>, lookback: Duration) -> Option<Position> {
        let mut position = Position {
            high_water: written.latest()?,
            seen: SeenEntities::default(),
            last_listed: cycle_start,
            missing_since: None,
        };
        position.list(written, cycle_start, lookback);
        Some(position)
    }

    /// Moves the position past `written`, the entities the cycle that started at `cycle_start` wrote
    /// for the stream, which it listed.
    ///
    /// The high-water mark never moves back: a late upload is remembered as seen, behind it.
    pub(crate) fn list(&mut self, written: SeenEntities, cycle_start: DateTime<Utc>, lookback: Duration) {
        if let Some(latest) = written.latest() {
            self.high_water = self.high_water.max(latest);
        }
        self.seen.join(written);
        self.seen.keep_from(reach_back(self.high_water, lookback));
        self.last_listed = cycle_start;
        self.missing_since = None;
    }

    /// Marks the stream missing since the cycle that started at `cycle_start`, unless an earlier
    /// cycle already found it missing.
    pub(crate) fn miss(&mut self, cycle_start: DateTime<Utc>) {
        self.missing_since = self.missing_since.or(Some(cycle_start));
    }

    /// Sets the high-water mark to `high_water` and forgets the entities seen, so the stream is read
    /// again from there.
    pub(crate) fn rewind(&mut self, high_water: DateTime<Utc>) {
        self.high_water = high_water;
        self.seen = SeenEntities::default();
    }
}

#[cfg(test)]
mod tests {
    use crate::{position::Position, seen_entities::SeenEntities};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};

    const LOOKBACK: Duration = Duration::from_secs(3600);

    /// The instant `time` on 2026-11-12, in UTC.
    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
    }

    fn seen(observations: &[(i64, &str)]) -> SeenEntities {
        SeenEntities::from(
            observations
                .iter()
                .map(|(id, time)| (EntityId::Integer(*id), at(time)))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn position(high_water: &str, seen: SeenEntities, missing_since: Option<&str>) -> Position {
        Position {
            high_water: at(high_water),
            seen,
            last_listed: at("09:00"),
            missing_since: missing_since.map(at),
        }
    }

    #[test]
    fn readings_advance_the_high_water_mark_and_join_seen() {
        let mut held = position("09:00", seen(&[(7, "08:30")]), None);
        held.list(seen(&[(9, "08:45"), (10, "09:30")]), at("10:00"), LOOKBACK);

        assert_eq!(held.high_water(), at("09:30"));
        assert_eq!(held.seen(), &seen(&[(7, "08:30"), (9, "08:45"), (10, "09:30")]));
        assert_eq!(held.last_listed(), at("10:00"));
    }

    #[test]
    fn a_late_reading_keeps_the_high_water_mark() {
        let mut held = position("09:00", SeenEntities::default(), None);
        held.list(seen(&[(11, "08:50")]), at("10:00"), LOOKBACK);

        assert_eq!(held.high_water(), at("09:00"));
        assert!(held.seen().contains(&EntityId::Integer(11)));
    }

    #[test]
    fn a_listed_stream_without_readings_keeps_its_position() {
        let mut held = position("09:00", seen(&[(7, "08:30")]), Some("08:00"));
        held.list(SeenEntities::default(), at("10:00"), LOOKBACK);

        assert_eq!(
            held,
            Position {
                last_listed: at("10:00"),
                missing_since: None,
                ..position("09:00", seen(&[(7, "08:30")]), None)
            }
        );
    }

    #[test]
    fn seen_observations_behind_the_lookback_are_pruned() {
        let first = Position::first(seen(&[(1, "08:30"), (2, "09:10"), (3, "10:00")]), at("10:00"), LOOKBACK).unwrap();

        assert_eq!(first.high_water(), at("10:00"));
        assert_eq!(first.seen(), &seen(&[(2, "09:10"), (3, "10:00")]));
    }

    #[test]
    fn a_stream_without_readings_gets_no_first_position() {
        assert_eq!(Position::first(SeenEntities::default(), at("10:00"), LOOKBACK), None);
    }

    #[test]
    fn a_missing_stream_keeps_the_start_of_the_first_cycle_that_missed_it() {
        let mut held = position("09:00", SeenEntities::default(), None);
        held.miss(at("10:00"));
        held.miss(at("11:00"));

        assert_eq!(held.missing_since(), Some(at("10:00")));
    }

    #[test]
    fn a_rewound_position_forgets_what_it_has_seen() {
        let mut held = position("09:00", seen(&[(7, "08:30")]), Some("08:00"));
        held.rewind(at("06:00"));

        assert_eq!(
            held,
            Position {
                high_water: at("06:00"),
                ..position("09:00", SeenEntities::default(), Some("08:00"))
            }
        );
    }
}

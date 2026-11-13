use crate::{position::Position, positions::Positions, seen_observations::SeenObservations};
use cassiopeia_sensorthings::entity_id::EntityId;
use chrono::{DateTime, Utc};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    time::Duration,
};

/// What one cycle read through a cursor: each data stream its discovery listed, with the
/// Observations the cycle wrote for it.
///
/// An update is a value: it changes no store until it is committed, so a cycle that fails or is
/// cancelled leaves its cursor where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorUpdate {
    /// The start of the cycle.
    cycle_start: DateTime<Utc>,
    /// How far behind its high-water mark a position keeps the Observations it has seen.
    lookback: Duration,
    /// The Observations the cycle wrote, by listed data stream.
    listed: BTreeMap<EntityId, SeenObservations>,
}

impl CursorUpdate {
    /// The update of the cycle that started at `cycle_start`, of an input read with `lookback`,
    /// before it has listed any data stream.
    #[must_use]
    pub const fn new(cycle_start: DateTime<Utc>, lookback: Duration) -> CursorUpdate {
        CursorUpdate {
            cycle_start,
            lookback,
            listed: BTreeMap::new(),
        }
    }

    /// Records that discovery listed `datastream` and that the cycle wrote `written` for it, which
    /// may be none. Recording a data stream again adds to what was written for it.
    pub fn record(&mut self, datastream: EntityId, written: SeenObservations) {
        self.listed.entry(datastream).or_default().join(written);
    }

    /// `committed`, the positions a store holds, moved past this update.
    ///
    /// A listed data stream moves past what the cycle wrote, and gets a first position only when the
    /// cycle wrote for it. A held position whose data stream was not listed keeps its high-water mark
    /// and seen Observations, and is marked missing.
    pub(crate) fn apply(self, committed: Positions) -> Positions {
        let mut positions = BTreeMap::from(committed);
        positions
            .iter_mut()
            .filter(|(datastream, _)| !self.listed.contains_key(*datastream))
            .for_each(|(_, position)| position.miss(self.cycle_start));
        for (datastream, written) in self.listed {
            match positions.entry(datastream) {
                Entry::Occupied(mut held) => held.get_mut().list(written, self.cycle_start, self.lookback),
                Entry::Vacant(free) => {
                    if let Some(first) = Position::first(written, self.cycle_start, self.lookback) {
                        free.insert(first);
                    }
                }
            }
        }
        Positions::from(positions)
    }
}

#[cfg(test)]
mod tests {
    use crate::{cursor_update::CursorUpdate, positions::Positions, seen_observations::SeenObservations};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};

    const LOOKBACK: Duration = Duration::from_secs(3600);

    /// The instant `time` on 2026-11-12, in UTC.
    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
    }

    fn seen(observations: &[(i64, &str)]) -> SeenObservations {
        SeenObservations::from(
            observations
                .iter()
                .map(|(id, time)| (EntityId::Integer(*id), at(time)))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    /// The positions after a cycle that started at `cycle_start` listed each of `listed` with what
    /// it wrote for it.
    fn after(committed: Positions, cycle_start: &str, listed: Vec<(i64, SeenObservations)>) -> Positions {
        let mut update = CursorUpdate::new(at(cycle_start), LOOKBACK);
        for (datastream, written) in listed {
            update.record(EntityId::Integer(datastream), written);
        }
        update.apply(committed)
    }

    #[test]
    fn a_new_stream_without_readings_gets_no_position() {
        assert_eq!(
            after(Positions::default(), "10:00", vec![(5, SeenObservations::default())]),
            Positions::default()
        );
    }

    #[test]
    fn a_new_stream_s_high_water_is_its_latest_reading() {
        let positions = after(Positions::default(), "10:00", vec![(5, seen(&[(3, "09:10"), (4, "09:40")]))]);
        let position = positions.get(&EntityId::Integer(5)).unwrap();

        assert_eq!(position.high_water(), at("09:40"));
        assert_eq!(position.seen(), &seen(&[(3, "09:10"), (4, "09:40")]));
        assert_eq!(position.last_listed(), at("10:00"));
        assert_eq!(position.missing_since(), None);
    }

    #[test]
    fn an_unlisted_stream_is_marked_missing_once() {
        let committed = after(Positions::default(), "09:00", vec![(5, seen(&[(1, "08:50")])), (9, seen(&[(2, "08:55")]))]);
        let first_miss = after(committed.clone(), "10:00", vec![(9, SeenObservations::default())]);
        let second_miss = after(first_miss.clone(), "11:00", vec![(9, SeenObservations::default())]);

        for positions in [&first_miss, &second_miss] {
            let missing = positions.get(&EntityId::Integer(5)).unwrap();
            let held = committed.get(&EntityId::Integer(5)).unwrap();

            assert_eq!(missing.missing_since(), Some(at("10:00")));
            assert_eq!((missing.high_water(), missing.seen()), (held.high_water(), held.seen()));
            assert_eq!(missing.last_listed(), at("09:00"));
        }
        assert_eq!(second_miss.get(&EntityId::Integer(9)).unwrap().missing_since(), None);
    }

    #[test]
    fn a_listed_stream_clears_its_missing_mark() {
        let committed = after(Positions::default(), "09:00", vec![(5, seen(&[(1, "08:50")]))]);
        let missed = after(committed, "10:00", Vec::new());
        let listed = after(missed, "11:00", vec![(5, SeenObservations::default())]);

        assert_eq!(listed.get(&EntityId::Integer(5)).unwrap().missing_since(), None);
        assert_eq!(listed.get(&EntityId::Integer(5)).unwrap().last_listed(), at("11:00"));
    }

    #[test]
    fn a_stream_recorded_twice_keeps_both_writes() {
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(EntityId::Integer(5), seen(&[(1, "09:10")]));
        update.record(EntityId::Integer(5), seen(&[(2, "09:20")]));
        let positions = update.apply(Positions::default());

        assert_eq!(positions.get(&EntityId::Integer(5)).unwrap().seen(), &seen(&[(1, "09:10"), (2, "09:20")]));
    }
}

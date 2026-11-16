use crate::{position::Position, positions::Positions, seen_entities::SeenEntities};
use cassiopeia_sensorthings::{read_stream::ReadStream, stream_id::StreamId, stream_kind::StreamKind};
use chrono::{DateTime, Utc};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    time::Duration,
};

/// What one cycle read through a cursor: each stream it listed, with the entities the cycle wrote for
/// it: each data stream its discovery listed with its Observations, and, when the read follows them,
/// each Thing those data streams belong to with its `HistoricalLocations`.
///
/// An update is a value: it changes no store until it is committed, so a cycle that fails or is
/// cancelled leaves its cursor where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorUpdate {
    /// The start of the cycle.
    cycle_start: DateTime<Utc>,
    /// How far behind its high-water mark a position keeps the entities it has seen.
    lookback: Duration,
    /// The entities the cycle wrote, by listed stream.
    listed: BTreeMap<ReadStream, SeenEntities>,
    /// The kinds of data stream whose discovery a collection limit stopped before it listed every
    /// data stream of the kind the server serves.
    cut_short: BTreeSet<StreamKind>,
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
            cut_short: BTreeSet::new(),
        }
    }

    /// Records that a collection limit stopped the discovery of the data streams of `kind` before it
    /// listed every one, so no position of the kind, and no position of a Thing, is marked missing for
    /// not being listed.
    pub fn listing_cut_short(&mut self, kind: StreamKind) {
        self.cut_short.insert(kind);
    }

    /// The data streams of `committed` this update marks missing, with their positions, Datastreams
    /// first, each kind by ascending id: those discovery did not list, unless the discovery of their
    /// kind was cut short.
    ///
    /// A Thing that is no longer listed has its position marked missing too, but it is not among
    /// these: its data streams are.
    pub fn missing<'a>(&self, committed: &'a Positions) -> impl Iterator<Item = (&'a StreamId, &'a Position)> {
        committed
            .iter()
            .filter(|(stream, _)| self.marks_missing(stream))
            .filter_map(|(stream, position)| match stream {
                ReadStream::Data(datastream) => Some((datastream, position)),
                ReadStream::Thing(_) => None,
            })
    }

    /// Records that the cycle listed `stream` and wrote `written` for it, which may be none.
    /// Recording a stream again adds to what was written for it.
    pub fn record(&mut self, stream: ReadStream, written: SeenEntities) {
        self.listed.entry(stream).or_default().join(written);
    }

    /// `committed`, the positions a store holds, moved past this update.
    ///
    /// A listed stream moves past what the cycle wrote, and gets a first position only when the cycle
    /// wrote for it. A held position whose stream was not listed keeps its high-water mark and the
    /// entities it has seen, and is marked missing unless the discovery of its kind was cut short, or,
    /// for a Thing, any discovery was.
    #[must_use]
    pub fn apply(self, committed: Positions) -> Positions {
        let mut positions = BTreeMap::from(committed);
        positions
            .iter_mut()
            .filter(|(stream, _)| self.marks_missing(stream))
            .for_each(|(_, position)| position.miss(self.cycle_start));
        for (stream, written) in self.listed {
            match positions.entry(stream) {
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

    /// Whether this update marks the position of `stream` missing.
    fn marks_missing(&self, stream: &ReadStream) -> bool {
        let listed_whole = match stream {
            ReadStream::Data(datastream) => !self.cut_short.contains(&datastream.kind()),
            // A Thing is listed through its data streams, which may be of either kind.
            ReadStream::Thing(_) => self.cut_short.is_empty(),
        };
        listed_whole && !self.listed.contains_key(stream)
    }
}

#[cfg(test)]
mod tests {
    use crate::{cursor_update::CursorUpdate, positions::Positions, seen_entities::SeenEntities};
    use cassiopeia_sensorthings::{entity_id::EntityId, read_stream::ReadStream, stream_id::StreamId, stream_kind::StreamKind};
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

    /// The Datastream `id`.
    fn datastream_id(id: i64) -> ReadStream {
        ReadStream::Data(StreamId::Datastream(EntityId::Integer(id)))
    }

    /// The `MultiDatastream` `id`.
    fn multi_datastream_id(id: i64) -> ReadStream {
        ReadStream::Data(StreamId::MultiDatastream(EntityId::Integer(id)))
    }

    /// The Thing `id`.
    fn thing_id(id: i64) -> ReadStream {
        ReadStream::Thing(EntityId::Integer(id))
    }

    /// The positions after a cycle that started at `cycle_start` listed each of `listed` with what
    /// it wrote for it.
    fn after(committed: Positions, cycle_start: &str, listed: Vec<(i64, SeenEntities)>) -> Positions {
        let mut update = CursorUpdate::new(at(cycle_start), LOOKBACK);
        for (datastream, written) in listed {
            update.record(datastream_id(datastream), written);
        }
        update.apply(committed)
    }

    #[test]
    fn a_new_stream_without_readings_gets_no_position() {
        assert_eq!(after(Positions::default(), "10:00", vec![(5, SeenEntities::default())]), Positions::default());
    }

    #[test]
    fn a_new_stream_s_high_water_is_its_latest_reading() {
        let positions = after(Positions::default(), "10:00", vec![(5, seen(&[(3, "09:10"), (4, "09:40")]))]);
        let position = positions.get(&datastream_id(5)).unwrap();

        assert_eq!(position.high_water(), at("09:40"));
        assert_eq!(position.seen(), &seen(&[(3, "09:10"), (4, "09:40")]));
        assert_eq!(position.last_listed(), at("10:00"));
        assert_eq!(position.missing_since(), None);
    }

    #[test]
    fn an_unlisted_stream_is_marked_missing_once() {
        let committed = after(Positions::default(), "09:00", vec![(5, seen(&[(1, "08:50")])), (9, seen(&[(2, "08:55")]))]);
        let first_miss = after(committed.clone(), "10:00", vec![(9, SeenEntities::default())]);
        let second_miss = after(first_miss.clone(), "11:00", vec![(9, SeenEntities::default())]);

        for positions in [&first_miss, &second_miss] {
            let missing = positions.get(&datastream_id(5)).unwrap();
            let held = committed.get(&datastream_id(5)).unwrap();

            assert_eq!(missing.missing_since(), Some(at("10:00")));
            assert_eq!((missing.high_water(), missing.seen()), (held.high_water(), held.seen()));
            assert_eq!(missing.last_listed(), at("09:00"));
        }
        assert_eq!(second_miss.get(&datastream_id(9)).unwrap().missing_since(), None);
    }

    #[test]
    fn a_listed_stream_clears_its_missing_mark() {
        let committed = after(Positions::default(), "09:00", vec![(5, seen(&[(1, "08:50")]))]);
        let missed = after(committed, "10:00", Vec::new());
        let listed = after(missed, "11:00", vec![(5, SeenEntities::default())]);

        assert_eq!(listed.get(&datastream_id(5)).unwrap().missing_since(), None);
        assert_eq!(listed.get(&datastream_id(5)).unwrap().last_listed(), at("11:00"));
    }

    #[test]
    fn a_cut_short_listing_marks_no_stream_missing() {
        let committed = after(Positions::default(), "09:00", vec![(5, seen(&[(1, "08:50")])), (9, seen(&[(2, "08:55")]))]);
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(datastream_id(5), SeenEntities::default());
        update.listing_cut_short(StreamKind::Datastream);

        assert_eq!(update.missing(&committed).count(), 0);
        let positions = update.apply(committed);
        assert_eq!(positions.get(&datastream_id(9)).unwrap().missing_since(), None);
        assert_eq!(positions.get(&datastream_id(9)).unwrap().last_listed(), at("09:00"));
    }

    #[test]
    fn the_positions_an_update_marks_missing_are_the_unlisted_ones() {
        let committed = after(Positions::default(), "09:00", vec![(5, seen(&[(1, "08:50")])), (9, seen(&[(2, "08:55")]))]);
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(datastream_id(9), SeenEntities::default());

        let missing: Vec<&StreamId> = update.missing(&committed).map(|(datastream, _)| datastream).collect();

        assert_eq!(missing, [&StreamId::Datastream(EntityId::Integer(5))]);
    }

    #[test]
    fn a_stream_recorded_twice_keeps_both_writes() {
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(datastream_id(5), seen(&[(1, "09:10")]));
        update.record(datastream_id(5), seen(&[(2, "09:20")]));
        let positions = update.apply(Positions::default());

        assert_eq!(positions.get(&datastream_id(5)).unwrap().seen(), &seen(&[(1, "09:10"), (2, "09:20")]));
    }

    #[test]
    fn a_datastream_and_a_multi_datastream_with_one_id_keep_a_position_each() {
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(datastream_id(4), seen(&[(1, "09:10")]));
        update.record(multi_datastream_id(4), seen(&[(2, "09:40")]));
        update.record(thing_id(4), seen(&[(3, "09:50")]));
        let positions = update.apply(Positions::default());

        assert_eq!(positions.len(), 3);
        assert_eq!(positions.get(&datastream_id(4)).unwrap().high_water(), at("09:10"));
        assert_eq!(positions.get(&multi_datastream_id(4)).unwrap().high_water(), at("09:40"));
        assert_eq!(positions.get(&thing_id(4)).unwrap().high_water(), at("09:50"));
    }

    #[test]
    fn a_listing_cut_short_for_one_kind_still_marks_the_other_kind_missing() {
        let mut first = CursorUpdate::new(at("09:00"), LOOKBACK);
        first.record(datastream_id(5), seen(&[(1, "08:50")]));
        first.record(multi_datastream_id(4), seen(&[(2, "08:55")]));
        let committed = first.apply(Positions::default());
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.listing_cut_short(StreamKind::MultiDatastream);

        let missing: Vec<&StreamId> = update.missing(&committed).map(|(datastream, _)| datastream).collect();

        assert_eq!(missing, [&StreamId::Datastream(EntityId::Integer(5))]);
    }

    #[test]
    fn a_thing_moves_past_its_historical_locations_and_is_marked_missing_but_not_reported_once_unlisted() {
        let mut first = CursorUpdate::new(at("09:00"), LOOKBACK);
        first.record(datastream_id(5), seen(&[(1, "08:50")]));
        first.record(thing_id(12512), seen(&[(88, "08:04"), (89, "08:34")]));
        let committed = first.apply(Positions::default());
        let held = committed.get(&thing_id(12512)).unwrap();
        assert_eq!(held.high_water(), at("08:34"));
        assert_eq!(held.seen(), &seen(&[(88, "08:04"), (89, "08:34")]));

        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(datastream_id(5), SeenEntities::default());
        let missing: Vec<&StreamId> = update.missing(&committed).map(|(datastream, _)| datastream).collect();
        let positions = update.apply(committed.clone());

        assert_eq!(missing, Vec::<&StreamId>::new());
        let unlisted = positions.get(&thing_id(12512)).unwrap();
        assert_eq!(unlisted.missing_since(), Some(at("10:00")));
        assert_eq!((unlisted.high_water(), unlisted.seen()), (held.high_water(), held.seen()));
    }

    #[test]
    fn a_cut_short_listing_of_either_kind_marks_no_thing_missing() {
        let mut first = CursorUpdate::new(at("09:00"), LOOKBACK);
        first.record(thing_id(12512), seen(&[(88, "08:04")]));
        let committed = first.apply(Positions::default());

        for kind in [StreamKind::Datastream, StreamKind::MultiDatastream] {
            let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
            update.listing_cut_short(kind);

            assert_eq!(update.apply(committed.clone()).get(&thing_id(12512)).unwrap().missing_since(), None, "{kind}");
        }
    }
}

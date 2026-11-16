use cassiopeia_sensorthings::{cursor_name::CursorName, service_root::ServiceRoot, stream_id::StreamId};
use cassiopeia_sensorthings_cursor::{cursor_update::CursorUpdate, positions::Positions};
use chrono::{DateTime, Utc};
use getset::{CopyGetters, Getters};
use std::num::NonZeroUsize;

/// How many missing data streams a warning names; it counts the rest.
const NAMED: usize = 3;

/// The data streams an incremental read keeps a position for that its server no longer lists, as
/// one cycle found them.
///
/// Their positions are kept, so a data stream the server lists again resumes where it stopped.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct DatastreamsMissing {
    /// The service root the read was built on.
    #[getset(get = "pub")]
    root: ServiceRoot,
    /// The cursor that keeps the positions.
    #[getset(get = "pub")]
    cursor: CursorName,
    /// The first missing data streams, Datastreams first, each kind by ascending id: at most three.
    #[getset(get = "pub")]
    named: Vec<StreamId>,
    /// How many data streams are missing.
    #[getset(get_copy = "pub")]
    count: NonZeroUsize,
    /// The earliest cycle start since which one of them is missing.
    #[getset(get_copy = "pub")]
    since: DateTime<Utc>,
}

impl DatastreamsMissing {
    /// The data streams of `committed`, the positions the cursor `cursor` of a read of `root` held,
    /// that `update`, the update of the cycle that started at `cycle_start`, marks missing; `None`
    /// when it marks none.
    ///
    /// A data stream missing since an earlier cycle is missing since that cycle's start, and one
    /// missing first in this cycle since `cycle_start`.
    #[must_use]
    pub fn of(root: &ServiceRoot, cursor: &CursorName, committed: &Positions, update: &CursorUpdate, cycle_start: DateTime<Utc>) -> Option<DatastreamsMissing> {
        let mut named = Vec::with_capacity(NAMED);
        let mut count: usize = 0;
        let mut since = cycle_start;
        for (datastream, position) in update.missing(committed) {
            if named.len() < NAMED {
                // The warning names the data stream, while the positions keep its id.
                named.push(datastream.clone());
            }
            count += 1;
            since = since.min(position.missing_since().unwrap_or(cycle_start));
        }
        Some(DatastreamsMissing {
            // The warning names the root and the cursor, while the read keeps its own.
            root: root.clone(),
            cursor: cursor.clone(),
            named,
            count: NonZeroUsize::new(count)?,
            since,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::datastreams_missing::DatastreamsMissing;
    use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, read_stream::ReadStream, service_root::ServiceRoot, stream_id::StreamId};
    use cassiopeia_sensorthings_cursor::{cursor_update::CursorUpdate, positions::Positions, seen_entities::SeenEntities};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};
    use url::Url;

    const LOOKBACK: Duration = Duration::from_secs(3600);

    /// The instant `time` on 2026-11-12, in UTC.
    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
    }

    fn root() -> ServiceRoot {
        ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap()
    }

    /// The Datastream `id`.
    fn stream(id: i64) -> StreamId {
        StreamId::Datastream(EntityId::Integer(id))
    }

    fn cursor() -> CursorName {
        CursorName::try_from("rivm-air".to_owned()).unwrap()
    }

    /// The update of a cycle that started at `cycle_start` and listed `listed`, writing nothing.
    fn listing(cycle_start: &str, listed: &[i64]) -> CursorUpdate {
        let mut update = CursorUpdate::new(at(cycle_start), LOOKBACK);
        for datastream in listed {
            update.record(ReadStream::Data(stream(*datastream)), SeenEntities::default());
        }
        update
    }

    /// The positions of data streams `datastreams`, each written once at 08:00 in a cycle that started
    /// at 09:00.
    fn positions(datastreams: &[i64]) -> Positions {
        let mut update = CursorUpdate::new(at("09:00"), LOOKBACK);
        for datastream in datastreams {
            let written = SeenEntities::from(BTreeMap::from([(EntityId::Integer(*datastream), at("08:00"))]));
            update.record(ReadStream::Data(stream(*datastream)), written);
        }
        update.apply(Positions::default())
    }

    #[test]
    fn every_listed_stream_is_no_missing_stream() {
        assert_eq!(
            DatastreamsMissing::of(&root(), &cursor(), &positions(&[5]), &listing("10:00", &[5]), at("10:00")),
            None
        );
    }

    #[test]
    fn missing_streams_are_named_up_to_three_and_counted() {
        let missing = DatastreamsMissing::of(&root(), &cursor(), &positions(&[1, 2, 3, 4, 5]), &listing("10:00", &[]), at("10:00")).unwrap();

        assert_eq!(missing.root(), &root());
        assert_eq!(missing.cursor(), &cursor());
        assert_eq!(missing.named(), &[stream(1), stream(2), stream(3)]);
        assert_eq!(missing.count().get(), 5);
        assert_eq!(missing.since(), at("10:00"));
    }

    #[test]
    fn a_stream_missing_since_an_earlier_cycle_dates_the_warning() {
        let missed = listing("10:00", &[9]).apply(positions(&[5, 9]));

        let missing = DatastreamsMissing::of(&root(), &cursor(), &missed, &listing("11:00", &[]), at("11:00")).unwrap();

        assert_eq!(missing.named(), &[stream(5), stream(9)]);
        assert_eq!(missing.since(), at("10:00"));
    }

    #[test]
    fn missing_multi_datastreams_are_named_after_missing_datastreams() {
        let mut update = CursorUpdate::new(at("09:00"), LOOKBACK);
        let written = |id: i64| SeenEntities::from(BTreeMap::from([(EntityId::Integer(id), at("08:00"))]));
        update.record(ReadStream::Data(StreamId::MultiDatastream(EntityId::Integer(4))), written(40));
        update.record(ReadStream::Data(stream(4)), written(41));
        let held = update.apply(Positions::default());

        let missing = DatastreamsMissing::of(&root(), &cursor(), &held, &listing("10:00", &[]), at("10:00")).unwrap();

        assert_eq!(missing.named(), &[stream(4), StreamId::MultiDatastream(EntityId::Integer(4))]);
        assert_eq!(missing.count().get(), 2);
    }
}

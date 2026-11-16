use crate::{datastream_span::ended_before, read_window::ReadWindow, stream_id::StreamId};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, vec};

/// The data streams an incremental read requests the Observations of, each with the window it
/// reads, gathered one served data stream at a time from its metadata discoveries.
///
/// Each data stream is read from its own start to the end every read of the cycle shares, the cycle
/// start. They are requested stalest first, by ascending start whatever their kind, then
/// Datastreams before `MultiDatastreams` and by ascending id, each once however the server ordered
/// or repeated them, so a cycle that `limits.observations` cuts short has read the data streams
/// furthest behind. A data stream whose own `phenomenonTime` ends before its start holds no reading
/// to read and is not requested, and neither is one whose start is not before the end.
#[derive(Debug)]
pub struct IncrementalDatastreams {
    /// The first `phenomenonTime` past every window.
    to: DateTime<Utc>,
    /// The window of each data stream requested.
    windows: BTreeMap<StreamId, ReadWindow>,
}

impl IncrementalDatastreams {
    /// No data stream yet, each to be read up to `to`.
    #[must_use]
    pub const fn new(to: DateTime<Utc>) -> IncrementalDatastreams {
        IncrementalDatastreams { to, windows: BTreeMap::new() }
    }

    /// Adds the data stream `id`, served as `datastream`, to be read from `from`, unless it holds no
    /// reading from there on.
    pub fn add(&mut self, id: StreamId, datastream: &Map<String, Value>, from: DateTime<Utc>) {
        if ended_before(datastream, from) {
            return;
        }
        if let Ok(window) = ReadWindow::new(from, self.to) {
            self.windows.insert(id, window);
        }
    }

    /// The data streams to request, each with its window, stalest first.
    #[must_use]
    pub fn into_reads(self) -> vec::IntoIter<(StreamId, ReadWindow)> {
        let mut reads: Vec<(StreamId, ReadWindow)> = self.windows.into_iter().collect();
        // The map yields Datastreams first, each kind by ascending id, and the sort is stable, so
        // data streams of one start keep that order.
        reads.sort_by_key(|(_, window)| window.from());
        reads.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, incremental_datastreams::IncrementalDatastreams, stream_id::StreamId, stream_kind::StreamKind};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    /// The data streams requested of `served`, each a Datastream with the start it is read from, as
    /// their ids and starts in the order requested, in a cycle that started at 2026-11-12T10:00:00Z.
    fn requested(served: &[(Value, &str)]) -> Vec<(EntityId, DateTime<Utc>)> {
        let streams: Vec<(StreamKind, Value, &str)> = served
            .iter()
            .map(|(datastream, from)| (StreamKind::Datastream, datastream.clone(), *from))
            .collect();
        requested_streams(&streams)
            .into_iter()
            .map(|(stream, from)| (stream.id().clone(), from))
            .collect()
    }

    /// The data streams requested of `served`, each a data stream of a kind with the start it is read
    /// from, with their starts in the order requested, in a cycle that started at
    /// 2026-11-12T10:00:00Z.
    fn requested_streams(served: &[(StreamKind, Value, &str)]) -> Vec<(StreamId, DateTime<Utc>)> {
        let mut datastreams = IncrementalDatastreams::new(instant("2026-11-12T10:00:00Z"));
        for (kind, datastream, from) in served {
            let datastream = object(datastream.clone());
            datastreams.add(StreamId::of(*kind, &datastream).unwrap(), &datastream, instant(from));
        }
        datastreams
            .into_reads()
            .map(|(id, window)| {
                assert_eq!(window.to(), instant("2026-11-12T10:00:00Z"));
                (id, window.from())
            })
            .collect()
    }

    #[test]
    fn the_stalest_stream_is_read_first_and_streams_of_one_start_by_id() {
        let reads = requested(&[
            (json!({"@iot.id": 9}), "2026-11-12T08:30:00Z"),
            (json!({"@iot.id": 7}), "2026-11-12T06:00:00Z"),
            (json!({"@iot.id": 5}), "2026-11-12T08:30:00Z"),
            (json!({"@iot.id": 3}), "2026-11-11T10:00:00Z"),
        ]);

        assert_eq!(
            reads,
            [
                (EntityId::Integer(3), instant("2026-11-11T10:00:00Z")),
                (EntityId::Integer(7), instant("2026-11-12T06:00:00Z")),
                (EntityId::Integer(5), instant("2026-11-12T08:30:00Z")),
                (EntityId::Integer(9), instant("2026-11-12T08:30:00Z")),
            ]
        );
    }

    #[test]
    fn a_stream_served_twice_is_read_once() {
        let reads = requested(&[(json!({"@iot.id": 5}), "2026-11-12T08:00:00Z"), (json!({"@iot.id": 5}), "2026-11-12T08:00:00Z")]);

        assert_eq!(reads, [(EntityId::Integer(5), instant("2026-11-12T08:00:00Z"))]);
    }

    #[test]
    fn a_stream_that_ended_before_its_start_is_not_read() {
        let reads = requested(&[
            (
                json!({"@iot.id": 1, "phenomenonTime": "2026-01-01T00:00:00Z/2026-11-12T07:00:00Z"}),
                "2026-11-12T08:00:00Z",
            ),
            (
                json!({"@iot.id": 2, "phenomenonTime": "2026-01-01T00:00:00Z/2026-11-12T09:00:00Z"}),
                "2026-11-12T08:00:00Z",
            ),
        ]);

        assert_eq!(reads, [(EntityId::Integer(2), instant("2026-11-12T08:00:00Z"))]);
    }

    #[test]
    fn a_stream_whose_start_is_not_before_the_cycle_start_is_not_read() {
        let reads = requested(&[(json!({"@iot.id": 1}), "2026-11-12T10:00:00Z"), (json!({"@iot.id": 2}), "2026-11-12T11:00:00Z")]);

        assert_eq!(reads, []);
    }

    #[test]
    fn data_streams_of_both_kinds_are_read_stalest_first_and_datastreams_first_on_a_tie() {
        let reads = requested_streams(&[
            (StreamKind::Datastream, json!({"@iot.id": 4}), "2026-11-12T08:00:00Z"),
            (StreamKind::MultiDatastream, json!({"@iot.id": 4}), "2026-11-12T06:00:00Z"),
            (StreamKind::MultiDatastream, json!({"@iot.id": 2}), "2026-11-12T08:00:00Z"),
        ]);

        assert_eq!(
            reads,
            [
                (StreamId::MultiDatastream(EntityId::Integer(4)), instant("2026-11-12T06:00:00Z")),
                (StreamId::Datastream(EntityId::Integer(4)), instant("2026-11-12T08:00:00Z")),
                (StreamId::MultiDatastream(EntityId::Integer(2)), instant("2026-11-12T08:00:00Z")),
            ]
        );
    }
}

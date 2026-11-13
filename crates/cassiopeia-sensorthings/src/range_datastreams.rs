use crate::{entity_id::EntityId, phenomenon_time::PhenomenonTime, read_window::ReadWindow};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, btree_set};

/// The member of a Datastream that holds the interval its Observations cover (OGC 18-088 section
/// 8.2.4, Table 10).
const PHENOMENON_TIME: &str = "phenomenonTime";

/// The data streams a range read requests the Observations of, gathered one served Datastream at a
/// time from its metadata discovery.
///
/// They are requested by ascending id, each once, however the server ordered or repeated them. A
/// Datastream's `phenomenonTime` covers all its Observations, so one that ends before the window
/// holds no reading of it and is not requested; one without a readable `phenomenonTime` is. A
/// Datastream without a readable id cannot be addressed, so it is not requested either, and the
/// ingestor refuses its line, naming it.
#[derive(Debug, Default)]
pub struct RangeDatastreams {
    /// The ids of the data streams requested.
    ids: BTreeSet<EntityId>,
}

impl RangeDatastreams {
    /// Adds the Datastream `datastream` as the server served it, unless it holds no reading of
    /// `window`.
    pub fn add(&mut self, datastream: &Map<String, Value>, window: ReadWindow) {
        if let Some(id) = EntityId::of(datastream)
            && !ended_before(datastream, window)
        {
            self.ids.insert(id);
        }
    }

    /// The ids of the data streams to request, ascending.
    #[must_use]
    pub fn into_ids(self) -> btree_set::IntoIter<EntityId> {
        self.ids.into_iter()
    }
}

/// Whether `datastream` states a `phenomenonTime` that ends before `window` starts.
fn ended_before(datastream: &Map<String, Value>, window: ReadWindow) -> bool {
    datastream
        .get(PHENOMENON_TIME)
        .and_then(Value::as_str)
        .and_then(|time| time.parse::<PhenomenonTime>().ok())
        .is_some_and(|time| time.instant() < window.from())
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, range_datastreams::RangeDatastreams, read_window::ReadWindow};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    /// The window from 2026-11-11T10:00:00Z to 2026-11-12T10:00:00Z.
    fn window() -> ReadWindow {
        let instant = |text: &str| -> DateTime<Utc> { DateTime::parse_from_rfc3339(text).unwrap().to_utc() };
        ReadWindow::new(instant("2026-11-11T10:00:00Z"), instant("2026-11-12T10:00:00Z")).unwrap()
    }

    /// The ids requested of the Datastreams `served`, in the order requested.
    fn requested(served: &[Value]) -> Vec<EntityId> {
        let mut datastreams = RangeDatastreams::default();
        for datastream in served {
            datastreams.add(&object(datastream.clone()), window());
        }
        datastreams.into_ids().collect()
    }

    #[test]
    fn data_streams_are_requested_by_ascending_id_once_each() {
        let ids = requested(&[
            json!({"@iot.id": 9}),
            json!({"@iot.id": 5}),
            json!({"@iot.id": "t-1"}),
            json!({"@iot.id": 7}),
            json!({"@iot.id": 5}),
        ]);

        assert_eq!(
            ids,
            [EntityId::Integer(5), EntityId::Integer(7), EntityId::Integer(9), EntityId::Text("t-1".into())]
        );
    }

    #[test]
    fn a_stream_that_ended_before_the_window_is_not_requested() {
        let ids = requested(&[
            json!({"@iot.id": 1, "phenomenonTime": "2026-01-01T00:00:00Z/2026-01-02T00:00:00Z"}),
            json!({"@iot.id": 2, "phenomenonTime": "2026-01-01T00:00:00Z/2026-11-11T10:00:00Z"}),
            json!({"@iot.id": 3, "phenomenonTime": "2026-11-11T09:00:00Z"}),
        ]);

        assert_eq!(ids, [EntityId::Integer(2)]);
    }

    #[test]
    fn a_stream_without_a_readable_phenomenon_time_is_requested() {
        let ids = requested(&[
            json!({"@iot.id": 1}),
            json!({"@iot.id": 2, "phenomenonTime": null}),
            json!({"@iot.id": 3, "phenomenonTime": "2026-01-01/2026-01-02"}),
        ]);

        assert_eq!(ids, [EntityId::Integer(1), EntityId::Integer(2), EntityId::Integer(3)]);
    }

    #[test]
    fn a_stream_without_a_readable_id_is_not_requested() {
        assert_eq!(requested(&[json!({"name": "no id"}), json!({"@iot.id": 1.5})]), []);
    }
}

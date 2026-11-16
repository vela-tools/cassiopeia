use crate::{entity_id::EntityId, stream_id::StreamId, stream_kind::StreamKind};
use getset::Getters;
use serde::{Deserialize, Serialize, de::Error};
use serde_json::{Map, Value};
use strum::Display;

/// A line of a snapshot after its header.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SnapshotLine {
    /// A data stream as discovery served it.
    Datastream(DatastreamLine),
    /// An Observation as a range or incremental read served it.
    Observation(ObservationLine),
    /// A `HistoricalLocation` as a range or incremental read served it.
    HistoricalLocation(HistoricalLocationLine),
}

/// The kind of a line after the header, in the order a snapshot holds them: every data stream line
/// first, then every observation line, then every historicalLocation line. A kind is displayed with
/// its article, so a message can name a line of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
pub enum LineKind {
    /// A data stream line.
    #[strum(serialize = "a data stream")]
    Datastream,
    /// An observation line.
    #[strum(serialize = "an observation")]
    Observation,
    /// A historicalLocation line.
    #[strum(serialize = "a historicalLocation")]
    HistoricalLocation,
}

/// A data stream as discovery served it, with its expanded Thing (and the Thing's Locations), what
/// it observes, its `Sensor` and, in a latest read, its newest Observation.
///
/// The line is a member named after the data stream's kind that holds the data stream:
/// `{"datastream": {...}}` for a Datastream and `{"multiDatastream": {...}}` for a
/// `MultiDatastream`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DatastreamLine {
    /// A Datastream's JSON as the server sent it, with its `ObservedProperty`.
    Datastream(Map<String, Value>),
    /// A `MultiDatastream`'s JSON as the server sent it, with its `ObservedProperties`.
    MultiDatastream(Map<String, Value>),
}

/// An Observation as a range or incremental read served it, with the data stream it belongs to,
/// which a `Datastreams({id})/Observations` or `MultiDatastreams({id})/Observations` response does
/// not carry.
///
/// The line names the data stream by a member named after its kind, as
/// `{"observation": {...}, "datastream": 56776}` or `{"observation": {...}, "multiDatastream": 4}`.
#[derive(Debug, Clone, PartialEq, Getters, Serialize)]
pub struct ObservationLine {
    /// The Observation's JSON as the server sent it.
    #[getset(get = "pub")]
    observation: Map<String, Value>,
    /// The data stream the Observation belongs to.
    #[getset(get = "pub")]
    #[serde(flatten)]
    datastream: StreamId,
}

/// A `HistoricalLocation` as a range or incremental read served it, with its Locations, and the id of
/// the Thing it belongs to, which a `Things({id})/HistoricalLocations` response does not carry.
///
/// The line is `{"historicalLocation": {...}, "thing": 12512}`.
#[derive(Debug, Clone, PartialEq, Getters, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalLocationLine {
    /// The `HistoricalLocation`'s JSON as the server sent it.
    #[getset(get = "pub")]
    historical_location: Map<String, Value>,
    /// The id of the Thing the `HistoricalLocation` belongs to.
    #[getset(get = "pub")]
    thing: EntityId,
}

/// A line after the header as it is read: an observation line names its data stream by id under
/// the member of its kind, while a data stream line holds the data stream itself under that member,
/// and a historicalLocation line names its Thing by id under `thing`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RawLine {
    observation: Option<Map<String, Value>>,
    datastream: Option<Value>,
    multi_datastream: Option<Value>,
    historical_location: Option<Map<String, Value>>,
    thing: Option<EntityId>,
}

impl SnapshotLine {
    /// The kind of the line.
    #[must_use]
    pub const fn kind(&self) -> LineKind {
        match self {
            SnapshotLine::Datastream(_) => LineKind::Datastream,
            SnapshotLine::Observation(_) => LineKind::Observation,
            SnapshotLine::HistoricalLocation(_) => LineKind::HistoricalLocation,
        }
    }
}

impl DatastreamLine {
    /// A line for a data stream of kind `kind` as the server sent it.
    #[must_use]
    pub const fn new(kind: StreamKind, served: Map<String, Value>) -> DatastreamLine {
        match kind {
            StreamKind::Datastream => DatastreamLine::Datastream(served),
            StreamKind::MultiDatastream => DatastreamLine::MultiDatastream(served),
        }
    }

    /// The kind of the data stream the line holds.
    #[must_use]
    pub const fn kind(&self) -> StreamKind {
        match self {
            DatastreamLine::Datastream(_) => StreamKind::Datastream,
            DatastreamLine::MultiDatastream(_) => StreamKind::MultiDatastream,
        }
    }

    /// The data stream's JSON as the server sent it.
    #[must_use]
    pub const fn datastream(&self) -> &Map<String, Value> {
        match self {
            DatastreamLine::Datastream(served) | DatastreamLine::MultiDatastream(served) => served,
        }
    }

    /// Consumes the line and returns the data stream's kind and JSON.
    #[must_use]
    pub fn into_parts(self) -> (StreamKind, Map<String, Value>) {
        match self {
            DatastreamLine::Datastream(served) => (StreamKind::Datastream, served),
            DatastreamLine::MultiDatastream(served) => (StreamKind::MultiDatastream, served),
        }
    }
}

impl ObservationLine {
    /// A line for an Observation as the server sent it, which belongs to the data stream `datastream`.
    #[must_use]
    pub const fn new(observation: Map<String, Value>, datastream: StreamId) -> ObservationLine {
        ObservationLine { observation, datastream }
    }

    /// Consumes the line and returns the Observation's JSON and its data stream.
    #[must_use]
    pub fn into_parts(self) -> (Map<String, Value>, StreamId) {
        (self.observation, self.datastream)
    }
}

impl HistoricalLocationLine {
    /// A line for a `HistoricalLocation` as the server sent it, which belongs to the Thing `thing`.
    #[must_use]
    pub const fn new(historical_location: Map<String, Value>, thing: EntityId) -> HistoricalLocationLine {
        HistoricalLocationLine { historical_location, thing }
    }

    /// Consumes the line and returns the `HistoricalLocation`'s JSON and the id of its Thing.
    #[must_use]
    pub fn into_parts(self) -> (Map<String, Value>, EntityId) {
        (self.historical_location, self.thing)
    }
}

impl TryFrom<RawLine> for SnapshotLine {
    type Error = serde_json::Error;

    fn try_from(raw: RawLine) -> Result<SnapshotLine, serde_json::Error> {
        let RawLine {
            observation,
            datastream,
            multi_datastream,
            historical_location,
            thing,
        } = raw;
        match (historical_location, thing) {
            (None, None) => stream_line(observation, datastream, multi_datastream),
            (Some(historical_location), Some(thing)) => match (observation, datastream, multi_datastream) {
                (None, None, None) => Ok(SnapshotLine::HistoricalLocation(HistoricalLocationLine::new(historical_location, thing))),
                (Some(_), _, _) | (_, Some(_), _) | (_, _, Some(_)) => Err(serde_json::Error::custom(
                    "a historicalLocation line holds a HistoricalLocation and the id of its Thing alone",
                )),
            },
            (Some(_), None) => Err(serde_json::Error::custom("a historicalLocation line names its Thing as `thing`")),
            (None, Some(_)) => Err(serde_json::Error::custom("only a historicalLocation line names a `thing`")),
        }
    }
}

/// The data stream line or observation line whose members are `observation`, `datastream` and
/// `multi_datastream`.
fn stream_line(observation: Option<Map<String, Value>>, datastream: Option<Value>, multi_datastream: Option<Value>) -> Result<SnapshotLine, serde_json::Error> {
    match (observation, datastream, multi_datastream) {
        (Some(observation), Some(id), None) => observation_line(observation, StreamKind::Datastream, &id),
        (Some(observation), None, Some(id)) => observation_line(observation, StreamKind::MultiDatastream, &id),
        (None, Some(Value::Object(served)), None) => Ok(SnapshotLine::Datastream(DatastreamLine::Datastream(served))),
        (None, None, Some(Value::Object(served))) => Ok(SnapshotLine::Datastream(DatastreamLine::MultiDatastream(served))),
        (_, Some(_), Some(_)) => Err(serde_json::Error::custom(
            "a line names one data stream, as `datastream` or as `multiDatastream`",
        )),
        (Some(_), None, None) => Err(serde_json::Error::custom(
            "an observation line names its data stream as `datastream` or `multiDatastream`",
        )),
        (None, Some(_), None) | (None, None, Some(_)) => Err(serde_json::Error::custom("a data stream line holds the data stream as a JSON object")),
        (None, None, None) => Err(serde_json::Error::custom("a line holds a data stream, an observation or a historicalLocation")),
    }
}

/// The observation line of `observation`, which names the data stream of kind `kind` whose id is
/// `id`.
fn observation_line(observation: Map<String, Value>, kind: StreamKind, id: &Value) -> Result<SnapshotLine, serde_json::Error> {
    Ok(SnapshotLine::Observation(ObservationLine::new(
        observation,
        StreamId::new(kind, EntityId::deserialize(id)?),
    )))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        snapshot::line::{DatastreamLine, HistoricalLocationLine, LineKind, ObservationLine, RawLine, SnapshotLine},
        stream_id::StreamId,
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    fn read(text: &str) -> Result<SnapshotLine, serde_json::Error> {
        serde_json::from_str::<RawLine>(text).and_then(SnapshotLine::try_from)
    }

    #[test]
    fn data_stream_lines_are_written_under_the_member_of_their_kind() {
        let datastream = SnapshotLine::Datastream(DatastreamLine::new(StreamKind::Datastream, object(json!({"@iot.id": 5}))));
        let multi = SnapshotLine::Datastream(DatastreamLine::new(StreamKind::MultiDatastream, object(json!({"@iot.id": 4}))));

        assert_eq!(serde_json::to_value(&datastream).unwrap(), json!({"datastream": {"@iot.id": 5}}));
        assert_eq!(serde_json::to_value(&multi).unwrap(), json!({"multiDatastream": {"@iot.id": 4}}));
    }

    #[test]
    fn observation_lines_name_their_data_stream_under_the_member_of_its_kind() {
        let observation = |stream: StreamId| SnapshotLine::Observation(ObservationLine::new(object(json!({"@iot.id": 501})), stream));

        assert_eq!(
            serde_json::to_string(&observation(StreamId::Datastream(EntityId::Integer(5)))).unwrap(),
            r#"{"observation":{"@iot.id":501},"datastream":5}"#
        );
        assert_eq!(
            serde_json::to_string(&observation(StreamId::MultiDatastream(EntityId::Integer(4)))).unwrap(),
            r#"{"observation":{"@iot.id":501},"multiDatastream":4}"#
        );
    }

    #[test]
    fn every_kind_of_line_is_read_back() {
        assert_eq!(
            read(r#"{"multiDatastream": {"@iot.id": 4}}"#).unwrap(),
            SnapshotLine::Datastream(DatastreamLine::MultiDatastream(object(json!({"@iot.id": 4}))))
        );
        assert_eq!(
            read(r#"{"observation": {"@iot.id": 501}, "multiDatastream": "ws-7"}"#).unwrap(),
            SnapshotLine::Observation(ObservationLine::new(
                object(json!({"@iot.id": 501})),
                StreamId::MultiDatastream(EntityId::Text("ws-7".into()))
            ))
        );
        assert_eq!(
            read(r#"{"observation": {"@iot.id": 501}, "datastream": 5}"#).unwrap(),
            SnapshotLine::Observation(ObservationLine::new(
                object(json!({"@iot.id": 501})),
                StreamId::Datastream(EntityId::Integer(5))
            ))
        );
    }

    #[test]
    fn historical_location_lines_name_their_thing_as_thing() {
        let line = SnapshotLine::HistoricalLocation(HistoricalLocationLine::new(
            object(json!({"@iot.id": 88, "time": "2026-11-12T10:04:00Z"})),
            EntityId::Integer(12512),
        ));

        assert_eq!(
            serde_json::to_string(&line).unwrap(),
            r#"{"historicalLocation":{"@iot.id":88,"time":"2026-11-12T10:04:00Z"},"thing":12512}"#
        );
        assert_eq!(
            read(r#"{"historicalLocation": {"@iot.id": 88}, "thing": "bus-12"}"#).unwrap(),
            SnapshotLine::HistoricalLocation(HistoricalLocationLine::new(object(json!({"@iot.id": 88})), EntityId::Text("bus-12".into())))
        );
    }

    #[test]
    fn a_historical_location_line_hands_back_its_parts() {
        let line = HistoricalLocationLine::new(object(json!({"@iot.id": 88})), EntityId::Integer(12512));

        assert_eq!(line.historical_location(), &object(json!({"@iot.id": 88})));
        assert_eq!(line.thing(), &EntityId::Integer(12512));
        assert_eq!(line.into_parts(), (object(json!({"@iot.id": 88})), EntityId::Integer(12512)));
    }

    #[test]
    fn a_historical_location_line_without_its_thing_or_with_more_is_refused() {
        for text in [
            r#"{"historicalLocation": {}}"#,
            r#"{"thing": 12512}"#,
            r#"{"historicalLocation": {}, "thing": 1.5}"#,
            r#"{"historicalLocation": {}, "thing": 12512, "datastream": 5}"#,
            r#"{"historicalLocation": {}, "thing": 12512, "observation": {}}"#,
            r#"{"observation": {}, "datastream": 5, "thing": 12512}"#,
        ] {
            assert!(read(text).is_err(), "{text}");
        }
    }

    #[test]
    fn each_line_names_its_kind_in_the_order_a_snapshot_holds_them() {
        let datastream = SnapshotLine::Datastream(DatastreamLine::new(StreamKind::Datastream, Map::new()));
        let observation = SnapshotLine::Observation(ObservationLine::new(Map::new(), StreamId::Datastream(EntityId::Integer(5))));
        let historical = SnapshotLine::HistoricalLocation(HistoricalLocationLine::new(Map::new(), EntityId::Integer(12512)));

        assert_eq!(
            [datastream.kind(), observation.kind(), historical.kind()],
            [LineKind::Datastream, LineKind::Observation, LineKind::HistoricalLocation]
        );
        assert!(LineKind::Datastream < LineKind::Observation && LineKind::Observation < LineKind::HistoricalLocation);
        assert_eq!(LineKind::HistoricalLocation.to_string(), "a historicalLocation");
        assert_eq!(LineKind::Observation.to_string(), "an observation");
    }

    #[test]
    fn a_line_that_names_no_data_stream_or_two_is_refused() {
        for text in [
            r#"{"observation": {}}"#,
            r#"{"observation": {}, "datastream": 5, "multiDatastream": 4}"#,
            r#"{"datastream": {}, "multiDatastream": {}}"#,
            r#"{"multiDatastream": 4}"#,
            r#"{"observation": {}, "multiDatastream": 4.5}"#,
            "{}",
        ] {
            assert!(read(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_data_stream_line_hands_back_its_kind_and_json() {
        let line = DatastreamLine::new(StreamKind::MultiDatastream, object(json!({"@iot.id": 4})));

        assert_eq!(line.kind(), StreamKind::MultiDatastream);
        assert_eq!(line.datastream(), &object(json!({"@iot.id": 4})));
        assert_eq!(line.into_parts(), (StreamKind::MultiDatastream, object(json!({"@iot.id": 4}))));
    }
}

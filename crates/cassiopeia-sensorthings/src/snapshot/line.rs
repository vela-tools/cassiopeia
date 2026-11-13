use crate::entity_id::EntityId;
use getset::Getters;
use serde::{Deserialize, Serialize, de::Error};
use serde_json::{Map, Value};

/// A line of a snapshot after its header.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SnapshotLine {
    /// A Datastream as discovery served it.
    Datastream(DatastreamLine),
    /// An Observation as a range or incremental read served it.
    Observation(ObservationLine),
}

/// A Datastream as discovery served it, with its expanded Thing (and the Thing's Locations), its
/// `ObservedProperty` and, in a latest read, its newest Observation.
#[derive(Debug, Clone, PartialEq, Getters, Serialize)]
pub struct DatastreamLine {
    /// The Datastream's JSON as the server sent it.
    #[getset(get = "pub")]
    datastream: Map<String, Value>,
}

/// An Observation as a range or incremental read served it, with the id of its data stream, which a
/// `Datastreams({id})/Observations` response does not carry.
#[derive(Debug, Clone, PartialEq, Getters, Serialize)]
pub struct ObservationLine {
    /// The Observation's JSON as the server sent it.
    #[getset(get = "pub")]
    observation: Map<String, Value>,
    /// The id of the data stream the Observation belongs to.
    #[getset(get = "pub")]
    datastream: EntityId,
}

/// A line after the header as it is read: an `observation` line names its data stream by id, while
/// a `datastream` line holds the Datastream itself.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLine {
    observation: Option<Map<String, Value>>,
    datastream: Value,
}

impl DatastreamLine {
    /// A line for a Datastream as the server sent it.
    #[must_use]
    pub const fn new(datastream: Map<String, Value>) -> DatastreamLine {
        DatastreamLine { datastream }
    }

    /// Consumes the line and returns the Datastream's JSON.
    #[must_use]
    pub fn into_datastream(self) -> Map<String, Value> {
        self.datastream
    }
}

impl ObservationLine {
    /// A line for an Observation as the server sent it, which belongs to the data stream `datastream`.
    #[must_use]
    pub const fn new(observation: Map<String, Value>, datastream: EntityId) -> ObservationLine {
        ObservationLine { observation, datastream }
    }

    /// Consumes the line and returns the Observation's JSON and its data stream's id.
    #[must_use]
    pub fn into_parts(self) -> (Map<String, Value>, EntityId) {
        (self.observation, self.datastream)
    }
}

impl TryFrom<RawLine> for SnapshotLine {
    type Error = serde_json::Error;

    fn try_from(raw: RawLine) -> Result<SnapshotLine, serde_json::Error> {
        match (raw.observation, raw.datastream) {
            (Some(observation), datastream) => Ok(SnapshotLine::Observation(ObservationLine::new(
                observation,
                EntityId::deserialize(&datastream)?,
            ))),
            (None, Value::Object(datastream)) => Ok(SnapshotLine::Datastream(DatastreamLine::new(datastream))),
            (None, _) => Err(serde_json::Error::custom("a datastream line holds the Datastream as a JSON object")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        snapshot::line::{DatastreamLine, ObservationLine, RawLine, SnapshotLine},
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
        SnapshotLine::try_from(serde_json::from_str::<RawLine>(text)?)
    }

    #[test]
    fn a_datastream_line_holds_the_datastream_as_served() {
        let line = read(r#"{"datastream":{"@iot.id":56776,"name":"ILM_I68-13-pm10"}}"#).unwrap();

        assert_eq!(
            line,
            SnapshotLine::Datastream(DatastreamLine::new(object(json!({"@iot.id": 56776, "name": "ILM_I68-13-pm10"}))))
        );
    }

    #[test]
    fn an_observation_line_names_its_data_stream_by_id() {
        let line = read(r#"{"observation":{"@iot.id":98766,"result":19.8},"datastream":56776}"#).unwrap();

        assert_eq!(
            line,
            SnapshotLine::Observation(ObservationLine::new(
                object(json!({"@iot.id": 98766, "result": 19.8})),
                EntityId::Integer(56776)
            ))
        );
    }

    #[test]
    fn lines_are_written_as_they_are_read() {
        let observation = SnapshotLine::Observation(ObservationLine::new(object(json!({"@iot.id": 1})), EntityId::Text("ds".into())));

        assert_eq!(
            serde_json::to_string(&observation).unwrap(),
            r#"{"observation":{"@iot.id":1},"datastream":"ds"}"#
        );
    }

    #[test]
    fn a_line_of_an_unknown_kind_is_refused() {
        assert!(read(r#"{"thing":{}}"#).is_err());
        assert!(read(r#"{"datastream":5}"#).is_err());
        assert!(read(r#"{"observation":{},"datastream":1.5}"#).is_err());
        assert!(read(r#"{"datastream":{},"extra":1}"#).is_err());
    }
}

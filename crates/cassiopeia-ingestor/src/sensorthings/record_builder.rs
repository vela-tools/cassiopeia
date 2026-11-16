use crate::sensorthings::{
    error::{SensorThingsIngestError, StaEntity},
    feature_of_interest::feature_of_interest,
    ingest_losses::SensorThingsLosses,
    null_members::move_member,
    served_id::optional_served_id,
};
use cassiopeia_ir::record::Record;
use cassiopeia_sensorthings::{
    entity_id::{EntityId, ID_MEMBER},
    phenomenon_time::{PHENOMENON_TIME_MEMBER, PhenomenonTime},
    service_root::ServiceRoot,
    snapshot::line_number::LineNumber,
};
use serde_json::{Map, Value};

/// The Observation members a reading carries as served, besides its id, result and times.
const OBSERVATION_MEMBERS: [&str; 4] = ["resultTime", "validTime", "resultQuality", "parameters"];

/// What a reading carries of the data stream it belongs to.
#[derive(Clone)]
pub(super) struct DatastreamParts {
    /// The reading's `unit`: the data stream's `unitOfMeasurement`, when it has one.
    pub(super) unit: Option<Value>,
    /// The reading's `datastream`: the data stream's id and members.
    pub(super) members: Map<String, Value>,
    /// The reading's `observedProperty`: the `ObservedProperty`'s id and members.
    pub(super) observed_property: Map<String, Value>,
    /// The reading's `sensor`: the `Sensor`'s id and members, when the data stream was served with
    /// one.
    pub(super) sensor: Option<Map<String, Value>>,
}

/// The members a record carries of a served entity: its `id`, then each of `keys` it has.
pub(super) fn record_members(mut served: Map<String, Value>, id: Option<&EntityId>, keys: &[&str]) -> Map<String, Value> {
    let mut members = Map::new();
    if let Some(id) = id {
        members.insert("id".to_owned(), id.json());
    }
    served.remove(ID_MEMBER);
    for key in keys {
        move_member(&mut members, key, &mut served, key);
    }
    members
}

/// A record's `server`: the service root, its host without port, and its version.
pub(super) fn server_member(root: &ServiceRoot) -> Value {
    let mut server = Map::new();
    server.insert("url".to_owned(), Value::from(root.to_string()));
    if let Some(host) = root.host() {
        server.insert("host".to_owned(), Value::from(host));
    }
    server.insert("version".to_owned(), Value::from(root.version().to_string()));
    Value::Object(server)
}

/// A record of one Thing: the `server`, the `thing`, its `location` when it has one, and its
/// `readings` when there are any.
pub(super) fn thing_record(server: Value, thing: Map<String, Value>, location: Option<Value>, readings: Map<String, Value>) -> Record {
    let mut data = Map::new();
    data.insert("server".to_owned(), server);
    data.insert("thing".to_owned(), Value::Object(thing));
    if let Some(location) = location {
        data.insert("location".to_owned(), location);
    }
    if !readings.is_empty() {
        data.insert("readings".to_owned(), Value::Object(readings));
    }
    Record::new(None, data)
}

/// The reading of one served Observation, read on `line`, with its `featureOfInterest` when the read
/// expanded it and what it carries of its data stream, its `sensor` among them, or `None` when its
/// `phenomenonTime` names no instant, which is counted as a loss.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the Observation or its `FeatureOfInterest`
/// has an id that is neither an integer nor a text.
pub(super) fn reading(
    mut observation: Map<String, Value>,
    line: LineNumber,
    datastream: DatastreamParts,
    losses: &mut SensorThingsLosses,
) -> Result<Option<Value>, SensorThingsIngestError> {
    let id = optional_served_id(&observation, line, StaEntity::Observation)?;
    let (text, time) = match observation.remove(PHENOMENON_TIME_MEMBER) {
        Some(Value::String(text)) => match text.parse::<PhenomenonTime>() {
            Ok(time) => (text, time),
            Err(unreadable) => {
                losses.unreadable_phenomenon_time(unreadable.into_text());
                return Ok(None);
            }
        },
        other => {
            // A missing time is written as the `null` it was read as.
            losses.unreadable_phenomenon_time(other.unwrap_or(Value::Null).to_string().into());
            return Ok(None);
        }
    };

    let mut reading = Map::new();
    if let Some(id) = id {
        reading.insert("id".to_owned(), id.json());
    }
    observation.remove(ID_MEMBER);
    move_member(&mut reading, "result", &mut observation, "result");
    reading.insert("time".to_owned(), Value::from(time.instant_text()));
    reading.insert(PHENOMENON_TIME_MEMBER.to_owned(), Value::String(text));
    for member in OBSERVATION_MEMBERS {
        move_member(&mut reading, member, &mut observation, member);
    }
    if let Some(feature) = feature_of_interest(&mut observation, line, losses)? {
        reading.insert("featureOfInterest".to_owned(), feature);
    }
    if let Some(unit) = datastream.unit {
        reading.insert("unit".to_owned(), unit);
    }
    reading.insert("datastream".to_owned(), Value::Object(datastream.members));
    reading.insert("observedProperty".to_owned(), Value::Object(datastream.observed_property));
    if let Some(sensor) = datastream.sensor {
        reading.insert("sensor".to_owned(), Value::Object(sensor));
    }
    Ok(Some(Value::Object(reading)))
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        ingest_losses::SensorThingsLosses,
        record_builder::{DatastreamParts, reading, server_member},
    };
    use cassiopeia_sensorthings::{service_root::ServiceRoot, snapshot::line_number::LineNumber};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    fn parts() -> DatastreamParts {
        DatastreamParts {
            unit: None,
            members: object(json!({"id": 5})),
            observed_property: object(json!({"name": "pm10"})),
            sensor: None,
        }
    }

    #[test]
    fn the_server_names_its_root_host_and_version() {
        let root: ServiceRoot = serde_json::from_str(r#""https://api-samenmeten.rivm.nl/v1.0""#).unwrap();

        assert_eq!(
            server_member(&root),
            json!({"url": "https://api-samenmeten.rivm.nl/v1.0", "host": "api-samenmeten.rivm.nl", "version": "v1.0"})
        );
    }

    #[test]
    fn a_missing_phenomenon_time_drops_the_reading_and_is_counted_as_null() {
        let mut losses = SensorThingsLosses::default();

        let read = reading(object(json!({"@iot.id": 1, "result": 2})), LineNumber::FIRST, parts(), &mut losses).unwrap();

        assert_eq!(read, None);
        assert_eq!(losses.unreadable_phenomenon_times().named(), [Box::<str>::from("null")]);
    }

    #[test]
    fn served_members_beyond_the_reading_s_are_left_out() {
        let mut losses = SensorThingsLosses::default();
        let observation = object(json!({"@iot.id": 1, "@iot.selfLink": "x", "phenomenonTime": "2026-11-12T10:00:00Z", "result": 2}));

        let read = reading(observation, LineNumber::FIRST, parts(), &mut losses).unwrap().unwrap();

        assert_eq!(
            read,
            json!({
                "id": 1,
                "result": 2,
                "time": "2026-11-12T10:00:00Z",
                "phenomenonTime": "2026-11-12T10:00:00Z",
                "datastream": {"id": 5},
                "observedProperty": {"name": "pm10"},
            })
        );
    }

    #[test]
    fn the_sensor_follows_the_observed_property() {
        let mut losses = SensorThingsLosses::default();
        let observation = object(json!({"@iot.id": 1, "phenomenonTime": "2026-11-12T10:00:00Z", "result": 2}));
        let parts = DatastreamParts {
            sensor: Some(object(json!({"id": 7, "name": "PM-200"}))),
            ..parts()
        };

        let read = reading(observation, LineNumber::FIRST, parts, &mut losses).unwrap().unwrap();

        assert_eq!(read["sensor"], json!({"id": 7, "name": "PM-200"}));
        assert_eq!(
            read.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["id", "result", "time", "phenomenonTime", "datastream", "observedProperty", "sensor"]
        );
    }
}

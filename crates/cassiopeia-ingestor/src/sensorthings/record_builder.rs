use crate::sensorthings::{
    error::{SensorThingsIngestError, StaEntity},
    feature_of_interest::feature_of_interest,
    ingest_losses::{ObservationName, SensorThingsLosses},
    location::ThingLocations,
    null_members::{move_member, present},
    served_id::optional_served_id,
};
use cassiopeia_ir::record::Record;
use cassiopeia_sensorthings::{
    entity_id::{EntityId, ID_MEMBER},
    phenomenon_time::{PHENOMENON_TIME_MEMBER, PhenomenonTime},
    service_root::ServiceRoot,
    snapshot::line_number::LineNumber,
    stream_kind::StreamKind,
};
use serde_json::{Map, Value};

/// The Observation members a reading carries as served, besides its id, result and times.
const OBSERVATION_MEMBERS: [&str; 4] = ["resultTime", "validTime", "resultQuality", "parameters"];

/// What a reading carries of the data stream it belongs to.
#[derive(Clone)]
pub(super) struct DatastreamParts {
    /// The reading's `unit`: the data stream's unit of measurement for the reading, when it has one.
    pub(super) unit: Option<Value>,
    /// The kind of the data stream, which names the member `members` is carried as.
    pub(super) kind: StreamKind,
    /// The reading's `datastream`, or its `multiDatastream` for an element of a `MultiDatastream`:
    /// the data stream's id and members.
    pub(super) members: Map<String, Value>,
    /// The reading's `observedProperty`: the `ObservedProperty`'s id and members.
    pub(super) observed_property: Map<String, Value>,
    /// The reading's `sensor`: the `Sensor`'s id and members, when the data stream was served with
    /// one.
    pub(super) sensor: Option<Map<String, Value>>,
}

/// One served Observation as its readings carry it: its id, its result, and the members every
/// reading of it shares, read once however many readings it gives.
pub(super) struct ObservationParts {
    /// The Observation's id, when it was served with one.
    id: Option<EntityId>,
    /// The Observation's `result`, when it has one.
    result: Option<Value>,
    /// The members after a reading's `result`: its `time`, its `phenomenonTime`, the Observation
    /// members it carries as served, and its `featureOfInterest`.
    shared: Map<String, Value>,
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

/// What a record of a Thing holds beside the Thing's own parts.
pub(super) enum RecordContent {
    /// The record's `readings`: those of one Observation, or the newest of each of the Thing's data
    /// streams, left out when there are none.
    Readings(Map<String, Value>),
    /// The record's `historicalLocation`: one place the Thing was known at, and when.
    HistoricalLocation(Map<String, Value>),
}

/// A record of one Thing: the `server`, the `thing`, its `location` when it has one, its
/// `locations` when it has any, and then its `readings` when there are any, or its
/// `historicalLocation`.
pub(super) fn thing_record(server: Value, thing: Map<String, Value>, locations: ThingLocations, content: RecordContent) -> Record {
    let mut data = Map::new();
    data.insert("server".to_owned(), server);
    data.insert("thing".to_owned(), Value::Object(thing));
    if let Some(location) = locations.location {
        data.insert("location".to_owned(), location);
    }
    if !locations.locations.is_empty() {
        data.insert("locations".to_owned(), Value::Array(locations.locations));
    }
    match content {
        RecordContent::Readings(readings) => {
            if !readings.is_empty() {
                data.insert("readings".to_owned(), Value::Object(readings));
            }
        }
        RecordContent::HistoricalLocation(historical_location) => {
            data.insert("historicalLocation".to_owned(), Value::Object(historical_location));
        }
    }
    Record::new(None, data)
}

impl ObservationParts {
    /// Reads the served `observation`, read on `line`, with its `featureOfInterest` when the read
    /// expanded it, or returns `None` when its `phenomenonTime` names no instant, which is counted
    /// as a loss.
    ///
    /// # Errors
    ///
    /// Returns [`SensorThingsIngestError::IdUnreadable`] when the Observation or its
    /// `FeatureOfInterest` has an id that is neither an integer nor a text.
    pub(super) fn read(
        mut observation: Map<String, Value>,
        line: LineNumber,
        losses: &mut SensorThingsLosses,
    ) -> Result<Option<ObservationParts>, SensorThingsIngestError> {
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

        observation.remove(ID_MEMBER);
        let result = observation.remove("result").and_then(present);
        let mut shared = Map::new();
        shared.insert("time".to_owned(), Value::from(time.instant_text()));
        shared.insert(PHENOMENON_TIME_MEMBER.to_owned(), Value::String(text));
        for member in OBSERVATION_MEMBERS {
            move_member(&mut shared, member, &mut observation, member);
        }
        if let Some(feature) = feature_of_interest(&mut observation, line, losses)? {
            shared.insert("featureOfInterest".to_owned(), feature);
        }
        Ok(Some(ObservationParts { id, result, shared }))
    }

    /// How a loss names the Observation.
    pub(super) fn name(&self) -> ObservationName {
        // The loss outlives the Observation it names; an id is a small value.
        self.id.clone().map_or(ObservationName::WithoutId, ObservationName::Id)
    }

    /// Takes the Observation's `result` out, so the values of a `MultiDatastream`'s result can be
    /// read one by one.
    pub(super) const fn take_result(&mut self) -> Option<Value> {
        self.result.take()
    }

    /// The reading of a Datastream's Observation: the Observation with its result and what it
    /// carries of `datastream`.
    pub(super) fn into_reading(self, datastream: DatastreamParts) -> Value {
        reading(self.id.as_ref(), self.result, self.shared, datastream)
    }

    /// The reading of one element of a `MultiDatastream`'s Observation: `result`, the value at the
    /// element's position, with the members every element shares and what it carries of `element`.
    pub(super) fn element_reading(&self, result: Value, element: DatastreamParts) -> Value {
        // Each element's reading owns the members the elements of one Observation share.
        reading(self.id.as_ref(), Some(result), self.shared.clone(), element)
    }
}

/// A reading: its `id`, its `result`, the members `shared` holds, and what it carries of the data
/// stream `datastream`, in that order.
fn reading(id: Option<&EntityId>, result: Option<Value>, shared: Map<String, Value>, datastream: DatastreamParts) -> Value {
    let mut reading = Map::new();
    if let Some(id) = id {
        reading.insert("id".to_owned(), id.json());
    }
    if let Some(result) = result {
        reading.insert("result".to_owned(), result);
    }
    reading.extend(shared);
    if let Some(unit) = datastream.unit {
        reading.insert("unit".to_owned(), unit);
    }
    reading.insert(stream_member(datastream.kind).to_owned(), Value::Object(datastream.members));
    reading.insert("observedProperty".to_owned(), Value::Object(datastream.observed_property));
    if let Some(sensor) = datastream.sensor {
        reading.insert("sensor".to_owned(), Value::Object(sensor));
    }
    Value::Object(reading)
}

/// The member a reading carries its data stream as: `datastream` for a Datastream, and
/// `multiDatastream` for an element of a `MultiDatastream`.
const fn stream_member(kind: StreamKind) -> &'static str {
    match kind {
        StreamKind::Datastream => "datastream",
        StreamKind::MultiDatastream => "multiDatastream",
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        ingest_losses::{ObservationName, SensorThingsLosses},
        record_builder::{DatastreamParts, ObservationParts, server_member},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, service_root::ServiceRoot, snapshot::line_number::LineNumber, stream_kind::StreamKind};
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
            kind: StreamKind::Datastream,
            members: object(json!({"id": 5})),
            observed_property: object(json!({"name": "pm10"})),
            sensor: None,
        }
    }

    /// The reading of the served Datastream Observation `observation` with `parts`, or `None` when it
    /// gives none.
    fn reading(observation: Map<String, Value>, parts: DatastreamParts, losses: &mut SensorThingsLosses) -> Option<Value> {
        ObservationParts::read(observation, LineNumber::FIRST, losses)
            .unwrap()
            .map(|read| read.into_reading(parts))
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

        let read = reading(object(json!({"@iot.id": 1, "result": 2})), parts(), &mut losses);

        assert_eq!(read, None);
        assert_eq!(losses.unreadable_phenomenon_times().named(), [Box::<str>::from("null")]);
    }

    #[test]
    fn served_members_beyond_the_reading_s_are_left_out() {
        let mut losses = SensorThingsLosses::default();
        let observation = object(json!({"@iot.id": 1, "@iot.selfLink": "x", "phenomenonTime": "2026-11-12T10:00:00Z", "result": 2}));

        let read = reading(observation, parts(), &mut losses).unwrap();

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

        let read = reading(observation, parts, &mut losses).unwrap();

        assert_eq!(read["sensor"], json!({"id": 7, "name": "PM-200"}));
        assert_eq!(
            read.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["id", "result", "time", "phenomenonTime", "datastream", "observedProperty", "sensor"]
        );
    }

    #[test]
    fn an_element_reading_carries_its_value_and_its_multi_datastream_after_the_shared_members() {
        let mut losses = SensorThingsLosses::default();
        let observation = object(json!({"@iot.id": 501, "phenomenonTime": "2026-11-12T10:00:00Z", "resultTime": "2026-11-12T10:00:01Z", "result": [4.2, 270]}));
        let mut read = ObservationParts::read(observation, LineNumber::FIRST, &mut losses).unwrap().unwrap();
        let element = DatastreamParts {
            unit: Some(json!({"symbol": "m/s"})),
            kind: StreamKind::MultiDatastream,
            members: object(json!({"id": 4, "name": "ws-7", "position": 1})),
            observed_property: object(json!({"name": "windSpeed"})),
            sensor: None,
        };

        assert_eq!(read.take_result(), Some(json!([4.2, 270])));
        let reading = read.element_reading(json!(4.2), element);

        assert_eq!(
            reading,
            json!({
                "id": 501,
                "result": 4.2,
                "time": "2026-11-12T10:00:00Z",
                "phenomenonTime": "2026-11-12T10:00:00Z",
                "resultTime": "2026-11-12T10:00:01Z",
                "unit": {"symbol": "m/s"},
                "multiDatastream": {"id": 4, "name": "ws-7", "position": 1},
                "observedProperty": {"name": "windSpeed"},
            })
        );
        assert_eq!(
            reading.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "id",
                "result",
                "time",
                "phenomenonTime",
                "resultTime",
                "unit",
                "multiDatastream",
                "observedProperty"
            ]
        );
        assert_eq!(read.name(), ObservationName::Id(EntityId::Integer(501)));
    }

    #[test]
    fn an_observation_without_an_id_is_named_as_one() {
        let mut losses = SensorThingsLosses::default();
        let read = ObservationParts::read(object(json!({"phenomenonTime": "2026-11-12T10:00:00Z"})), LineNumber::FIRST, &mut losses)
            .unwrap()
            .unwrap();

        assert_eq!(read.name(), ObservationName::WithoutId);
    }
}

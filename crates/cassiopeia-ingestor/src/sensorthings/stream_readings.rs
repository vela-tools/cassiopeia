use crate::sensorthings::{
    element_position::ElementPosition,
    error::{SensorThingsIngestError, StaEntity},
    null_members::present,
    reading_keys::ReadingSource,
    record_builder::{DatastreamParts, record_members},
    served_id::optional_served_id,
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    multi_datastream_elements::take_elements,
    snapshot::line_number::LineNumber,
    stream_id::StreamId,
    stream_kind::StreamKind,
};
use serde_json::{Map, Value};

/// The Datastream members a reading's `datastream` carries: its own, then the `phenomenonTime`,
/// `resultTime` and `observedArea` that sum up all of its Observations (OGC 18-088 section 8.2.4,
/// Table 10), as the server served them when it listed the Datastream.
const DATASTREAM_MEMBERS: [&str; 7] = [
    "name",
    "description",
    "observationType",
    "properties",
    "phenomenonTime",
    "resultTime",
    "observedArea",
];

/// The `MultiDatastream` members a reading's `multiDatastream` carries, before its element's
/// `observationType` and `position`: its own, then the `phenomenonTime`, `resultTime` and
/// `observedArea` that sum up all of its Observations (OGC 18-088 section 12, Table 26), as the
/// server served them when it listed the `MultiDatastream`.
const MULTI_DATASTREAM_MEMBERS: [&str; 6] = ["name", "description", "properties", "phenomenonTime", "resultTime", "observedArea"];

/// The `ObservedProperty` members a reading's `observedProperty` carries.
const OBSERVED_PROPERTY_MEMBERS: [&str; 4] = ["name", "definition", "description", "properties"];

/// One reading a data stream gives each of its Observations, before its key is known, since the key
/// depends on the other readings of its Thing.
pub(super) struct StreamReading {
    /// Where the reading comes from.
    pub(super) source: ReadingSource,
    /// The name of its `ObservedProperty`.
    pub(super) name: Box<str>,
    /// What it carries of its data stream.
    pub(super) parts: DatastreamParts,
}

/// The readings the data stream `datastream`, served as `served` on `line`, gives each of its
/// Observations, each carrying `sensor`: one for a Datastream, and one per element for a
/// `MultiDatastream`, in position order.
///
/// The data stream's Thing, `Sensor` and Observations are taken out of `served` before.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError`] when the data stream has no `ObservedProperty`, or one
/// without a `name` or with an unreadable id, and when a `MultiDatastream`'s `ObservedProperties`,
/// `unitOfMeasurements` and `multiObservationDataTypes` differ in number.
pub(super) fn stream_readings(
    datastream: &StreamId,
    served: Map<String, Value>,
    sensor: Option<Map<String, Value>>,
    line: LineNumber,
) -> Result<Vec<StreamReading>, SensorThingsIngestError> {
    match datastream {
        StreamId::Datastream(id) => Ok(vec![datastream_reading(datastream, id, served, sensor, line)?]),
        StreamId::MultiDatastream(id) => element_readings(datastream, id, served, sensor.as_ref(), line),
    }
}

/// The one reading of the Datastream `datastream`, whose id is `id`.
fn datastream_reading(
    datastream: &StreamId,
    id: &EntityId,
    mut served: Map<String, Value>,
    sensor: Option<Map<String, Value>>,
    line: LineNumber,
) -> Result<StreamReading, SensorThingsIngestError> {
    let Some(Value::Object(observed)) = served.remove("ObservedProperty") else {
        return Err(missing(datastream));
    };
    let (name, observed_property) = observed_property(observed, datastream, line)?;
    Ok(StreamReading {
        // The reading's source keeps the id, while the data stream keeps it to be found by; an id
        // is a small value.
        source: ReadingSource::Datastream(id.clone()),
        name,
        parts: DatastreamParts {
            unit: served.remove("unitOfMeasurement").and_then(present),
            kind: StreamKind::Datastream,
            members: record_members(served, Some(id), &DATASTREAM_MEMBERS),
            observed_property,
            sensor,
        },
    })
}

/// The readings of the elements of the `MultiDatastream` `datastream`, whose id is `id`, in position
/// order, each carrying a copy of `sensor`.
fn element_readings(
    datastream: &StreamId,
    id: &EntityId,
    mut served: Map<String, Value>,
    sensor: Option<&Map<String, Value>>,
    line: LineNumber,
) -> Result<Vec<StreamReading>, SensorThingsIngestError> {
    let elements = take_elements(&mut served).map_err(|counts| SensorThingsIngestError::ElementCountsDiffer {
        line,
        // The error outlives the line it names.
        multi_datastream: id.clone(),
        counts,
    })?;
    if elements.is_empty() {
        return Err(missing(datastream));
    }
    let members = record_members(served, Some(id), &MULTI_DATASTREAM_MEMBERS);
    elements
        .into_iter()
        .enumerate()
        .map(|(index, element)| {
            let position = ElementPosition::at_index(index);
            let Value::Object(observed) = element.observed_property else {
                return Err(missing(datastream));
            };
            let (name, observed_property) = observed_property(observed, datastream, line)?;
            // Each element's reading owns its copy of the members the elements share.
            let mut multi_datastream = members.clone();
            if let Some(observation_type) = present(element.observation_type) {
                multi_datastream.insert("observationType".to_owned(), observation_type);
            }
            multi_datastream.insert("position".to_owned(), position.json());
            Ok(StreamReading {
                // Each element's source keeps the id; an id is a small value.
                source: ReadingSource::Element(id.clone(), position),
                name,
                parts: DatastreamParts {
                    unit: present(element.unit),
                    kind: StreamKind::MultiDatastream,
                    members: multi_datastream,
                    observed_property,
                    // Each element's reading owns its copy of the Sensor the elements share.
                    sensor: sensor.cloned(),
                },
            })
        })
        .collect()
}

/// The name and the reading's `observedProperty` of `observed`, an `ObservedProperty` of the data
/// stream `datastream` served on `line`.
fn observed_property(observed: Map<String, Value>, datastream: &StreamId, line: LineNumber) -> Result<(Box<str>, Map<String, Value>), SensorThingsIngestError> {
    let Some(name) = observed.get("name").and_then(Value::as_str).map(Box::<str>::from) else {
        return Err(SensorThingsIngestError::ObservedPropertyUnnamed {
            // The error outlives the line it names.
            datastream: datastream.clone(),
        });
    };
    let id = optional_served_id(&observed, line, StaEntity::ObservedProperty)?;
    Ok((name, record_members(observed, id.as_ref(), &OBSERVED_PROPERTY_MEMBERS)))
}

/// The failure of the data stream `datastream`, served without an `ObservedProperty`.
fn missing(datastream: &StreamId) -> SensorThingsIngestError {
    SensorThingsIngestError::ObservedPropertyMissing {
        // The error outlives the line it names.
        datastream: datastream.clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        error::SensorThingsIngestError,
        reading_keys::ReadingSource,
        stream_readings::{StreamReading, stream_readings},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber, stream_id::StreamId, stream_kind::StreamKind};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    fn read(datastream: &StreamId, served: Value) -> Result<Vec<StreamReading>, SensorThingsIngestError> {
        stream_readings(datastream, object(served), Some(object(json!({"id": 7, "name": "WS-500"}))), LineNumber::FIRST)
    }

    /// A served `MultiDatastream` 4 with wind speed and wind direction.
    fn wind() -> Value {
        json!({
            "@iot.id": 4,
            "@iot.selfLink": "https://h.example/v1.1/MultiDatastreams(4)",
            "name": "ws-7",
            "description": "Wind at station 7",
            "properties": {"height": 10},
            "observationType": "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_ComplexObservation",
            "ObservedProperties": [{"@iot.id": 11, "name": "windSpeed"}, {"@iot.id": 12, "name": "windDirection", "definition": null}],
            "unitOfMeasurements": [{"name": "metre per second", "symbol": "m/s"}, {"name": "degree", "symbol": "deg"}],
            "multiObservationDataTypes": [
                "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_Measurement",
                "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_Measurement",
            ],
        })
    }

    #[test]
    fn a_datastream_gives_one_reading_named_after_its_observed_property() {
        let readings = read(
            &StreamId::Datastream(EntityId::Integer(5)),
            json!({"@iot.id": 5, "name": "pm10 stream", "unitOfMeasurement": {"symbol": "ug/m3"}, "ObservedProperty": {"@iot.id": 1, "name": "pm10"}}),
        )
        .unwrap();

        let [reading] = readings.as_slice() else {
            panic!("expected one reading");
        };
        assert_eq!(reading.source, ReadingSource::Datastream(EntityId::Integer(5)));
        assert_eq!(&*reading.name, "pm10");
        assert_eq!(reading.parts.kind, StreamKind::Datastream);
        assert_eq!(Value::Object(reading.parts.members.clone()), json!({"id": 5, "name": "pm10 stream"}));
        assert_eq!(reading.parts.unit, Some(json!({"symbol": "ug/m3"})));
    }

    #[test]
    fn a_multi_datastream_gives_one_reading_per_element_with_what_its_position_holds() {
        let readings = read(&StreamId::MultiDatastream(EntityId::Integer(4)), wind()).unwrap();

        let shown: Vec<(String, String, Value, Value, Value)> = readings
            .iter()
            .map(|reading| {
                (
                    reading.source.to_string(),
                    reading.name.to_string(),
                    reading.parts.unit.clone().unwrap_or(Value::Null),
                    Value::Object(reading.parts.members.clone()),
                    Value::Object(reading.parts.observed_property.clone()),
                )
            })
            .collect();
        let measurement = "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_Measurement";
        assert_eq!(
            shown,
            [
                (
                    "element 1 of MultiDatastream 4".to_owned(),
                    "windSpeed".to_owned(),
                    json!({"name": "metre per second", "symbol": "m/s"}),
                    json!({"id": 4, "name": "ws-7", "description": "Wind at station 7", "properties": {"height": 10}, "observationType": measurement, "position": 1}),
                    json!({"id": 11, "name": "windSpeed"}),
                ),
                (
                    "element 2 of MultiDatastream 4".to_owned(),
                    "windDirection".to_owned(),
                    json!({"name": "degree", "symbol": "deg"}),
                    json!({"id": 4, "name": "ws-7", "description": "Wind at station 7", "properties": {"height": 10}, "observationType": measurement, "position": 2}),
                    json!({"id": 12, "name": "windDirection"}),
                ),
            ]
        );
        assert!(
            readings
                .iter()
                .all(|reading| reading.parts.kind == StreamKind::MultiDatastream && reading.parts.sensor.is_some())
        );
    }

    /// The `phenomenonTime`, `resultTime` and `observedArea` of a data stream whose Observations were
    /// made from 1 to 12 November 2026 around Eindhoven.
    fn extents() -> Value {
        json!({
            "phenomenonTime": "2026-11-01T00:00:00Z/2026-11-12T10:00:00Z",
            "resultTime": "2026-11-01T00:00:05Z/2026-11-12T10:00:05Z",
            "observedArea": {"type": "Polygon", "coordinates": [[[5.40, 51.40], [5.50, 51.40], [5.50, 51.47], [5.40, 51.47], [5.40, 51.40]]]},
        })
    }

    /// `served` with the members of `extents`.
    fn with_extents(served: Value) -> Value {
        let mut served = object(served);
        served.extend(object(extents()));
        Value::Object(served)
    }

    #[test]
    fn a_datastream_carries_its_times_and_observed_area_after_its_own_members() {
        let served = with_extents(json!({"@iot.id": 5, "name": "pm10 stream", "properties": {"floor": 2}, "ObservedProperty": {"@iot.id": 1, "name": "pm10"}}));

        let readings = read(&StreamId::Datastream(EntityId::Integer(5)), served).unwrap();

        let mut expected = object(json!({"id": 5, "name": "pm10 stream", "properties": {"floor": 2}}));
        expected.extend(object(extents()));
        assert_eq!(Value::Object(readings[0].parts.members.clone()), Value::Object(expected));
        assert_eq!(
            readings[0].parts.members.keys().collect::<Vec<_>>(),
            ["id", "name", "properties", "phenomenonTime", "resultTime", "observedArea"]
        );
    }

    #[test]
    fn every_element_of_a_multi_datastream_carries_its_times_and_observed_area_before_its_position() {
        let readings = read(&StreamId::MultiDatastream(EntityId::Integer(4)), with_extents(wind())).unwrap();

        assert_eq!(readings.len(), 2);
        for reading in &readings {
            let members = &reading.parts.members;
            for (member, value) in object(extents()) {
                assert_eq!(members.get(&member), Some(&value), "{member} of {}", reading.source);
            }
            assert_eq!(
                members.keys().collect::<Vec<_>>(),
                [
                    "id",
                    "name",
                    "description",
                    "properties",
                    "phenomenonTime",
                    "resultTime",
                    "observedArea",
                    "observationType",
                    "position"
                ],
                "{}",
                reading.source
            );
        }
    }

    #[test]
    fn members_that_differ_in_number_fail_naming_the_multi_datastream() {
        let mut mismatched = wind();
        mismatched["unitOfMeasurements"] = json!([{"symbol": "m/s"}, {"symbol": "deg"}, {"symbol": "K"}]);

        let error = read(&StreamId::MultiDatastream(EntityId::Integer(4)), mismatched).err().unwrap();

        assert!(
            matches!(&error, SensorThingsIngestError::ElementCountsDiffer { multi_datastream, counts, .. } if *multi_datastream == EntityId::Integer(4) && counts.units == 3),
            "{error:?}"
        );
    }

    #[test]
    fn a_multi_datastream_without_observed_properties_or_with_an_unnamed_one_fails() {
        let stream = StreamId::MultiDatastream(EntityId::Integer(4));
        let mut empty = wind();
        empty["ObservedProperties"] = json!([]);
        empty["unitOfMeasurements"] = json!([]);
        empty["multiObservationDataTypes"] = json!([]);
        let mut unnamed = wind();
        unnamed["ObservedProperties"][1] = json!({"@iot.id": 12});

        assert!(matches!(
            read(&stream, empty),
            Err(SensorThingsIngestError::ObservedPropertyMissing { datastream }) if datastream == stream
        ));
        assert!(matches!(
            read(&stream, unnamed),
            Err(SensorThingsIngestError::ObservedPropertyUnnamed { datastream }) if datastream == stream
        ));
    }
}

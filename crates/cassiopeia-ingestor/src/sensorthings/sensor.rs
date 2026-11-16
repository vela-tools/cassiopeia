use crate::sensorthings::{
    error::{SensorThingsIngestError, StaEntity},
    record_builder::record_members,
    served_id::optional_served_id,
};
use cassiopeia_sensorthings::snapshot::line_number::LineNumber;
use serde_json::{Map, Value};

/// The member a discovery expands a Datastream's `Sensor` into.
const SENSOR_MEMBER: &str = "Sensor";

/// The `Sensor` members a reading's `sensor` carries besides its id (OGC 18-088 section 8.2.5).
/// `metadata` is kept as served, text or JSON, since what it points to, such as a datasheet or a
/// `SensorML` description, is never fetched.
const SENSOR_MEMBERS: [&str; 5] = ["name", "description", "encodingType", "metadata", "properties"];

/// A reading's `sensor`: the id and members of the `Sensor` the discovery expanded into the served
/// Datastream `datastream` on `line`, taken out of it, or `None` when the Datastream was served
/// without one.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the `Sensor` has an id that is neither an
/// integer nor a text.
pub(super) fn sensor(datastream: &mut Map<String, Value>, line: LineNumber) -> Result<Option<Map<String, Value>>, SensorThingsIngestError> {
    let Some(Value::Object(served)) = datastream.remove(SENSOR_MEMBER) else {
        return Ok(None);
    };
    let id = optional_served_id(&served, line, StaEntity::Sensor)?;
    Ok(Some(record_members(served, id.as_ref(), &SENSOR_MEMBERS)))
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        error::{SensorThingsIngestError, StaEntity},
        sensor::sensor,
    };
    use cassiopeia_sensorthings::snapshot::line_number::LineNumber;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    /// A served Datastream expanded with the `Sensor` `served`.
    fn datastream(served: Value) -> Map<String, Value> {
        let mut datastream = Map::new();
        datastream.insert("@iot.id".to_owned(), json!(5));
        datastream.insert("name".to_owned(), json!("station-1 pm10"));
        datastream.insert("Sensor".to_owned(), served);
        datastream
    }

    /// The `sensor` of a Datastream served with the `Sensor` `served`.
    fn read(served: Value) -> Result<Option<Value>, SensorThingsIngestError> {
        Ok(sensor(&mut datastream(served), LineNumber::FIRST)?.map(Value::Object))
    }

    /// The `sensor` read from `served`, which must be one.
    fn read_sensor(served: Value) -> Value {
        read(served).unwrap().expect("the Datastream has a Sensor")
    }

    #[test]
    fn a_sensor_gives_its_id_and_members_as_served() {
        let mut datastream = datastream(json!({
            "@iot.id": 7,
            "@iot.selfLink": "https://h.example/v1.1/Sensors(7)",
            "name": "PM-200",
            "description": "Optical particulate matter sensor",
            "encodingType": "application/pdf",
            "metadata": "https://sensors.example.org/pm-200.pdf",
            "properties": {"calibrated": "2026-10-01"},
            "Datastreams@iot.navigationLink": "https://h.example/v1.1/Sensors(7)/Datastreams",
        }));

        let read = sensor(&mut datastream, LineNumber::FIRST).unwrap();

        assert_eq!(
            read.map(Value::Object),
            Some(json!({
                "id": 7,
                "name": "PM-200",
                "description": "Optical particulate matter sensor",
                "encodingType": "application/pdf",
                "metadata": "https://sensors.example.org/pm-200.pdf",
                "properties": {"calibrated": "2026-10-01"},
            }))
        );
        assert_eq!(Value::Object(datastream), json!({"@iot.id": 5, "name": "station-1 pm10"}));
    }

    #[test]
    fn json_metadata_is_kept_as_served_without_its_null_members() {
        let sensor = read_sensor(json!({
            "@iot.id": "pm-200",
            "encodingType": "application/json",
            "metadata": {"datasheet": "https://sensors.example.org/pm-200.pdf", "range": [0, 1000], "firmware": null},
        }));

        assert_eq!(
            sensor,
            json!({
                "id": "pm-200",
                "encodingType": "application/json",
                "metadata": {"datasheet": "https://sensors.example.org/pm-200.pdf", "range": [0, 1000]},
            })
        );
    }

    #[test]
    fn null_members_are_left_out() {
        let sensor = read_sensor(json!({"@iot.id": 7, "name": "PM-200", "description": null, "metadata": null}));

        assert_eq!(sensor, json!({"id": 7, "name": "PM-200"}));
    }

    #[test]
    fn a_sensor_without_an_id_is_read_without_one() {
        assert_eq!(read_sensor(json!({"name": "PM-200"})), json!({"name": "PM-200"}));
    }

    #[test]
    fn a_datastream_served_without_a_sensor_has_none() {
        for served in [Value::Null, json!("https://h.example/v1.1/Sensors(7)"), json!([{"@iot.id": 7}])] {
            let shown = served.to_string();
            assert_eq!(read(served).unwrap(), None, "{shown}");
        }

        let Value::Object(mut datastream) = json!({"@iot.id": 5, "Sensor@iot.navigationLink": "https://h.example/v1.1/Datastreams(5)/Sensor"}) else {
            panic!("expected an object");
        };
        assert_eq!(sensor(&mut datastream, LineNumber::FIRST).unwrap(), None);
    }

    #[test]
    fn a_sensor_with_an_unreadable_id_fails() {
        for id in [json!(1.5), json!(true), json!({"id": 7})] {
            let mut served = json!({"name": "PM-200"});
            served["@iot.id"] = id;
            let read = read(served);
            assert!(
                matches!(read, Err(SensorThingsIngestError::IdUnreadable { entity: StaEntity::Sensor, .. })),
                "{read:?}"
            );
        }
    }
}

use crate::sensorthings::{
    error::{SensorThingsIngestError, StaEntity},
    ingest_losses::{HistoricalLocationFault, HistoricalLocationName, SensorThingsLosses, UnreadableHistoricalLocation},
    location::read_locations,
    served_id::optional_served_id,
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    historical_location_time::{TIME_MEMBER, historical_location_time},
    instant_text::instant_text,
    snapshot::line_number::LineNumber,
};
use serde_json::{Map, Value};

/// The member a served `HistoricalLocation` holds its Locations in (OGC 18-088 section 8.2.3).
const LOCATIONS_MEMBER: &str = "Locations";

/// The record's `historicalLocation` of the served `HistoricalLocation` `served` of the Thing `thing`,
/// read on `line`: its `id`, its `time` written as a reading's `time` is, and the geometry of its
/// Locations as `location`, chosen and read as a record's `location` is.
///
/// A `HistoricalLocation` whose `time` names no instant, or that has no Location or none in
/// `GeoJSON`, gives `None`, which is counted as a loss naming it, its Thing and what could not be
/// read. One whose `GeoJSON` Location with the lowest id holds no geometry gives a
/// `historicalLocation` without `location`, as a Thing's record is then given no `location`.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the `HistoricalLocation` has an id that is
/// neither an integer nor a text, or a Location has no readable id.
pub(super) fn historical_location(
    mut served: Map<String, Value>,
    thing: &EntityId,
    line: LineNumber,
    losses: &mut SensorThingsLosses,
) -> Result<Option<Map<String, Value>>, SensorThingsIngestError> {
    let id = optional_served_id(&served, line, StaEntity::HistoricalLocation)?;
    let fault = match historical_location_time(&served) {
        Some(time) => {
            let (locations, lowest) = read_locations(served.remove(LOCATIONS_MEMBER), line)?;
            match (lowest, locations.locations.is_empty()) {
                (None, false) => {
                    let mut historical_location = Map::new();
                    if let Some(id) = id {
                        historical_location.insert("id".to_owned(), id.json());
                    }
                    historical_location.insert("time".to_owned(), Value::from(instant_text(time)));
                    if let Some(location) = locations.location {
                        historical_location.insert("location".to_owned(), location);
                    }
                    return Ok(Some(historical_location));
                }
                (Some(lowest), _) => HistoricalLocationFault::NoGeoJsonLocation(lowest),
                (None, true) => HistoricalLocationFault::NoLocation,
            }
        }
        None => HistoricalLocationFault::Time(served_time(&served)),
    };
    losses.unreadable_historical_location(UnreadableHistoricalLocation {
        historical_location: id.map_or(HistoricalLocationName::WithoutId, HistoricalLocationName::Id),
        // The loss outlives the line it was read on; an id is a small value.
        thing: thing.clone(),
        fault,
    });
    Ok(None)
}

/// The `time` of `served` as a loss names it: its text, or the JSON of a `time` that is no text,
/// `null` when it has none.
fn served_time(served: &Map<String, Value>) -> Box<str> {
    match served.get(TIME_MEMBER) {
        Some(Value::String(text)) => text.as_str().into(),
        other => other.unwrap_or(&Value::Null).to_string().into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        historical_location::historical_location,
        ingest_losses::{GeometryEncoding, HistoricalLocationFault, HistoricalLocationName, LowestLocation, SensorThingsLosses, UnreadableHistoricalLocation},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    /// The Thing every test reads the `HistoricalLocations` of.
    const THING: EntityId = EntityId::Integer(12512);

    /// The `historicalLocation` of a `HistoricalLocation` served as `served`, and what reading it lost.
    fn read(served: Value) -> (Option<Value>, SensorThingsLosses) {
        let Value::Object(served) = served else {
            panic!("expected an object");
        };
        let mut losses = SensorThingsLosses::default();
        let read = historical_location(served, &THING, LineNumber::FIRST, &mut losses).unwrap();
        (read.map(Value::Object), losses)
    }

    /// A `GeoJSON` Location `id` of a Point at `coordinates`.
    fn geojson(id: i64, coordinates: [f64; 2]) -> Value {
        json!({"@iot.id": id, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": coordinates}})
    }

    /// The faults `losses` names and how many it counts.
    fn faults(losses: &SensorThingsLosses) -> (Vec<UnreadableHistoricalLocation>, u64) {
        let tally = losses.unreadable_historical_locations();
        (tally.named().to_vec(), tally.total())
    }

    #[test]
    fn a_historical_location_gives_its_id_its_time_in_utc_and_the_geometry_of_its_location() {
        let (read, losses) = read(json!({
            "@iot.id": 88,
            "@iot.selfLink": "https://h.example/v1.1/HistoricalLocations(88)",
            "time": "2026-11-12T12:04:00.250+02:00",
            "Locations": [geojson(36190, [5.4797, 51.4433])],
        }));

        assert_eq!(
            read,
            Some(json!({
                "id": 88,
                "time": "2026-11-12T10:04:00.250Z",
                "location": {"type": "Point", "coordinates": [5.4797, 51.4433]},
            }))
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn the_geojson_location_gives_the_geometry_though_a_text_location_has_a_lower_id() {
        let (read, losses) = read(json!({
            "@iot.id": 89,
            "time": "2026-11-12T10:34:00Z",
            "Locations": [
                geojson(36192, [5.4801, 51.441]),
                {"@iot.id": 36191, "encodingType": "text/plain", "location": "Stationsplein, platform 2"},
            ],
        }));

        assert_eq!(read.unwrap()["location"], json!({"type": "Point", "coordinates": [5.4801, 51.441]}));
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_time_that_names_no_instant_gives_no_record_and_a_loss() {
        for (time, text) in [
            (json!("2026-11-12"), "2026-11-12"),
            (json!("2026-11-12T10:04:00"), "2026-11-12T10:04:00"),
            (json!(null), "null"),
            (json!(1_762_941_840), "1762941840"),
        ] {
            let (read, losses) = read(json!({"@iot.id": 88, "time": time, "Locations": [geojson(36190, [5.0, 51.0])]}));

            assert_eq!(read, None, "{text}");
            assert_eq!(
                faults(&losses),
                (
                    vec![UnreadableHistoricalLocation {
                        historical_location: HistoricalLocationName::Id(EntityId::Integer(88)),
                        thing: THING,
                        fault: HistoricalLocationFault::Time(text.into()),
                    }],
                    1
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn a_historical_location_without_a_time_is_named_with_a_null_time() {
        let (read, losses) = read(json!({"Locations": [geojson(36190, [5.0, 51.0])]}));

        assert_eq!(read, None);
        assert_eq!(
            losses.unreadable_historical_locations().named()[0].to_string(),
            "a HistoricalLocation without an id of Thing 12512 has the `time` `null`, which names no instant"
        );
    }

    #[test]
    fn a_historical_location_without_a_geojson_location_gives_no_record_and_names_its_lowest_location() {
        let (read, losses) = read(json!({
            "@iot.id": 90,
            "time": "2026-11-12T11:04:00Z",
            "Locations": [
                {"@iot.id": 36194, "encodingType": "text/plain", "location": "Depot"},
                {"@iot.id": 36193, "encodingType": "text/plain", "location": "Stationsplein"},
            ],
        }));

        assert_eq!(read, None);
        assert_eq!(
            faults(&losses),
            (
                vec![UnreadableHistoricalLocation {
                    historical_location: HistoricalLocationName::Id(EntityId::Integer(90)),
                    thing: THING,
                    fault: HistoricalLocationFault::NoGeoJsonLocation(LowestLocation {
                        location: EntityId::Integer(36193),
                        encoding: GeometryEncoding::Stated("text/plain".into()),
                    }),
                }],
                1
            )
        );
    }

    #[test]
    fn a_historical_location_without_locations_gives_no_record() {
        for locations in [Some(json!([])), Some(json!(null)), None] {
            let mut served = Map::from_iter([("@iot.id".to_owned(), json!(91)), ("time".to_owned(), json!("2026-11-12T11:34:00Z"))]);
            if let Some(locations) = &locations {
                served.insert("Locations".to_owned(), locations.clone());
            }

            let (read, losses) = read(Value::Object(served));

            assert_eq!(read, None, "{locations:?}");
            assert_eq!(
                losses.unreadable_historical_locations().named()[0].fault,
                HistoricalLocationFault::NoLocation,
                "{locations:?}"
            );
        }
    }

    #[test]
    fn a_lowest_geojson_location_without_a_geometry_gives_a_historical_location_without_location() {
        let (read, losses) = read(json!({
            "@iot.id": 92,
            "time": "2026-11-12T12:04:00Z",
            "Locations": [{"@iot.id": 36195, "encodingType": "application/geo+json"}],
        }));

        assert_eq!(read, Some(json!({"id": 92, "time": "2026-11-12T12:04:00Z"})));
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_historical_location_without_an_id_gives_one_without_id() {
        let (read, _) = read(json!({"time": "2026-11-12T12:34:00Z", "Locations": [geojson(36190, [5.0, 51.0])]}));

        assert_eq!(
            read,
            Some(json!({"time": "2026-11-12T12:34:00Z", "location": {"type": "Point", "coordinates": [5.0, 51.0]}}))
        );
    }

    #[test]
    fn an_unreadable_id_fails() {
        let Value::Object(served) = json!({"@iot.id": 1.5, "time": "2026-11-12T12:34:00Z"}) else {
            panic!("expected an object");
        };

        assert!(historical_location(served, &THING, LineNumber::FIRST, &mut SensorThingsLosses::default()).is_err());
    }
}

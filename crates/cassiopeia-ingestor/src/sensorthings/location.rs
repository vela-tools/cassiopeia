use crate::sensorthings::{
    encoded_geometry::{EncodedGeometry, encoded_geometry},
    error::{SensorThingsIngestError, StaEntity},
    ingest_losses::{SensorThingsLosses, UnreadableLocation},
    null_members::present,
    record_builder::record_members,
    served_id::served_id,
};
use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
use serde_json::{Map, Value};

/// The member a Location holds its place in, and the member a record carries a place as.
const LOCATION_MEMBER: &str = "location";

/// The Location members a record's `locations` carries besides its id and its `location` (OGC
/// 18-088 section 8.2.2).
const LOCATION_MEMBERS: [&str; 4] = ["name", "description", "encodingType", "properties"];

/// What a record carries of a Thing's Locations.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ThingLocations {
    /// The record's `location`: the geometry of the Thing's `GeoJSON` Location with the lowest id,
    /// when that Location holds one.
    pub(super) location: Option<Value>,
    /// The record's `locations`: every Location of the Thing, in id order.
    pub(super) locations: Vec<Value>,
}

/// One served Location, read.
struct ReadLocation {
    /// The Location's id.
    id: EntityId,
    /// The Location's geometry when it is `GeoJSON`, or what it states about its encoding when not.
    geometry: EncodedGeometry,
    /// The Location's `location` as served, kept when it is not `GeoJSON`.
    served: Option<Value>,
    /// The Location's id and the members of [`LOCATION_MEMBERS`] it has.
    members: Map<String, Value>,
}

/// The Locations of the Thing `thing` whose served `Locations` are `locations`, read on `line`.
///
/// A Thing may have several Locations, each in its own encoding (OGC 18-088 section 8.2.1, Table 4).
/// Its `location` is the geometry of its `GeoJSON` Location with the lowest id, read by the rules of
/// [`encoded_geometry`], and a Thing that has Locations and none in `GeoJSON` is counted as a loss
/// naming its Location with the lowest id. Ids are ordered as [`EntityId`] orders them. A Thing
/// without Locations has neither.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when a Location has no readable id, since the
/// Locations cannot be ordered without it.
pub(super) fn thing_locations(
    locations: Option<Value>,
    thing: &EntityId,
    line: LineNumber,
    losses: &mut SensorThingsLosses,
) -> Result<ThingLocations, SensorThingsIngestError> {
    let Some(Value::Array(served)) = locations.and_then(present) else {
        return Ok(ThingLocations::default());
    };
    let mut read = served
        .into_iter()
        .map(|location| read_location(location, line))
        .collect::<Result<Vec<_>, _>>()?;
    // A stable sort keeps Locations that share an id in the order they were served.
    read.sort_by(|left, right| left.id.cmp(&right.id));

    // The lowest `GeoJSON` Location decides `location`, whether or not it holds a geometry.
    let lowest_geojson = read.iter().position(|location| !matches!(location.geometry, EncodedGeometry::Unreadable(_)));
    let mut location = None;
    let mut locations = Vec::with_capacity(read.len());
    for (
        position,
        ReadLocation {
            id,
            geometry,
            served,
            mut members,
        },
    ) in read.into_iter().enumerate()
    {
        match geometry {
            EncodedGeometry::Geometry(geometry) => {
                if Some(position) == lowest_geojson {
                    // The record carries this geometry twice: as its `location`, and in its
                    // `locations`.
                    location = Some(geometry.clone());
                }
                members.insert(LOCATION_MEMBER.to_owned(), geometry);
            }
            EncodedGeometry::Absent => {}
            EncodedGeometry::Unreadable(encoding) => {
                if let Some(served) = served {
                    members.insert(LOCATION_MEMBER.to_owned(), served);
                }
                if position == 0 && lowest_geojson.is_none() {
                    losses.unreadable_location(UnreadableLocation {
                        // The loss is reported after the Thing's own id has gone into its record.
                        thing: thing.clone(),
                        location: id,
                        encoding,
                    });
                }
            }
        }
        locations.push(Value::Object(members));
    }
    Ok(ThingLocations { location, locations })
}

/// Reads one served Location on `line`.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the Location is no object, or has no
/// readable id.
fn read_location(location: Value, line: LineNumber) -> Result<ReadLocation, SensorThingsIngestError> {
    let Value::Object(mut location) = location else {
        return Err(SensorThingsIngestError::IdUnreadable {
            line,
            entity: StaEntity::Location,
        });
    };
    let id = served_id(&location, line, StaEntity::Location)?;
    let geometry = encoded_geometry(&mut location, LOCATION_MEMBER);
    // A `GeoJSON` Location's member was taken out as its geometry; any other is left as served.
    let served = location.remove(LOCATION_MEMBER).and_then(present);
    let members = record_members(location, Some(&id), &LOCATION_MEMBERS);
    Ok(ReadLocation { id, geometry, served, members })
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        ingest_losses::{GeometryEncoding, SensorThingsLosses, UnreadableLocation},
        location::{ThingLocations, thing_locations},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// The Thing every test reads the Locations of.
    const THING: EntityId = EntityId::Integer(12512);

    /// The Locations of a Thing served with `locations`, and what reading them lost.
    fn read(locations: Value) -> (ThingLocations, SensorThingsLosses) {
        let mut losses = SensorThingsLosses::default();
        let read = thing_locations(Some(locations), &THING, LineNumber::FIRST, &mut losses).unwrap();
        (read, losses)
    }

    /// A Point at `coordinates`.
    fn point(coordinates: [f64; 2]) -> Value {
        json!({"type": "Point", "coordinates": coordinates})
    }

    #[test]
    fn the_geojson_location_gives_the_location_though_another_has_a_lower_id() {
        let (read, losses) = read(json!([
            {"@iot.id": 2, "encodingType": "application/geo+json", "location": point([5.4797, 51.4433])},
            {"@iot.id": 1, "encodingType": "text/plain", "location": "Stationsplein 1, 3rd floor, room 3.12"},
        ]));

        assert_eq!(read.location, Some(point([5.4797, 51.4433])));
        assert_eq!(
            read.locations,
            [
                json!({"id": 1, "encodingType": "text/plain", "location": "Stationsplein 1, 3rd floor, room 3.12"}),
                json!({"id": 2, "encodingType": "application/geo+json", "location": point([5.4797, 51.4433])}),
            ]
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn the_geojson_location_with_the_lowest_id_gives_the_location() {
        let (read, _) = read(json!([
            {"@iot.id": 3, "encodingType": "application/geo+json", "location": point([3.0, 3.0])},
            {"@iot.id": 2, "encodingType": "application/geo+json", "location": point([2.0, 2.0])},
        ]));

        assert_eq!(read.location, Some(point([2.0, 2.0])));
        assert_eq!(read.locations.iter().map(|location| &location["id"]).collect::<Vec<_>>(), [2, 3]);
    }

    #[test]
    fn a_location_gives_its_members_as_served() {
        let (read, _) = read(json!([{
            "@iot.id": 36182,
            "@iot.selfLink": "https://h.example/v1.1/Locations(36182)",
            "name": "Stationsplein 1, Eindhoven",
            "description": "Roof of the station building",
            "encodingType": "application/geo+json",
            "properties": {"floor": "roof"},
            "location": point([5.4797, 51.4433]),
            "Things@iot.navigationLink": "https://h.example/v1.1/Locations(36182)/Things",
        }]));

        assert_eq!(
            read.locations,
            [json!({
                "id": 36182,
                "name": "Stationsplein 1, Eindhoven",
                "description": "Roof of the station building",
                "encodingType": "application/geo+json",
                "properties": {"floor": "roof"},
                "location": point([5.4797, 51.4433]),
            })]
        );
        assert_eq!(
            read.locations[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["id", "name", "description", "encodingType", "properties", "location"]
        );
    }

    #[test]
    fn a_feature_location_yields_its_geometry_in_both_members() {
        let (read, _) = read(json!([{
            "@iot.id": "loc-1",
            "encodingType": "application/geo+json",
            "location": {"type": "Feature", "geometry": point([1.0, 2.0]), "properties": {}},
        }]));

        assert_eq!(read.location, Some(point([1.0, 2.0])));
        assert_eq!(read.locations[0]["location"], point([1.0, 2.0]));
    }

    #[test]
    fn the_older_geojson_media_type_is_read_in_any_case_and_with_parameters() {
        for encoding in ["application/vnd.geo+json", "Application/GEO+JSON; charset=utf-8"] {
            let (read, losses) = read(json!([{"@iot.id": 1, "encodingType": encoding, "location": point([5.608, 51.313])}]));
            assert_eq!(read.location, Some(point([5.608, 51.313])), "{encoding}");
            assert_eq!(losses, SensorThingsLosses::default(), "{encoding}");
        }
    }

    #[test]
    fn integer_ids_come_before_text_ids() {
        let (read, _) = read(json!([
            {"@iot.id": "b", "encodingType": "application/geo+json", "location": point([2.0, 2.0])},
            {"@iot.id": 7, "encodingType": "application/geo+json", "location": point([7.0, 7.0])},
            {"@iot.id": "a", "encodingType": "application/geo+json", "location": point([1.0, 1.0])},
        ]));

        assert_eq!(
            read.locations.iter().map(|location| &location["id"]).collect::<Vec<_>>(),
            [&json!(7), &json!("a"), &json!("b")]
        );
        assert_eq!(read.location, Some(point([7.0, 7.0])));
    }

    #[test]
    fn a_thing_without_a_geojson_location_has_none_and_names_its_lowest_location() {
        let (read, losses) = read(json!([
            {"@iot.id": 9, "encodingType": "application/gml+xml", "location": "<gml:Point/>"},
            {"@iot.id": 4, "encodingType": "text/plain", "location": "Valkenswaard"},
        ]));

        assert_eq!(read.location, None);
        assert_eq!(
            read.locations,
            [
                json!({"id": 4, "encodingType": "text/plain", "location": "Valkenswaard"}),
                json!({"id": 9, "encodingType": "application/gml+xml", "location": "<gml:Point/>"}),
            ]
        );
        assert_eq!(
            losses.unreadable_locations().named(),
            [UnreadableLocation {
                thing: THING,
                location: EntityId::Integer(4),
                encoding: GeometryEncoding::Stated("text/plain".into()),
            }]
        );
        assert_eq!(losses.unreadable_locations().total(), 1);
    }

    #[test]
    fn a_location_without_an_encoding_is_no_geojson() {
        let (read, losses) = read(json!([{"@iot.id": 1, "location": point([0.0, 0.0])}]));

        assert_eq!(read.location, None);
        assert_eq!(read.locations, [json!({"id": 1, "location": point([0.0, 0.0])})]);
        assert_eq!(losses.unreadable_locations().named()[0].encoding, GeometryEncoding::Unstated);
    }

    #[test]
    fn a_lowest_geojson_location_without_a_geometry_gives_no_location_and_no_loss() {
        let (read, losses) = read(json!([
            {"@iot.id": 1, "encodingType": "application/geo+json", "location": null},
            {"@iot.id": 2, "encodingType": "application/geo+json", "location": point([2.0, 2.0])},
        ]));

        assert_eq!(read.location, None);
        assert_eq!(read.locations[0], json!({"id": 1, "encodingType": "application/geo+json"}));
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn null_members_are_left_out() {
        let (read, _) = read(json!([{
            "@iot.id": 1,
            "name": "Station building",
            "description": null,
            "encodingType": "text/plain",
            "properties": {"room": null},
            "location": "Stationsplein 1",
        }]));

        assert_eq!(
            read.locations,
            [json!({"id": 1, "name": "Station building", "encodingType": "text/plain", "properties": {}, "location": "Stationsplein 1"})]
        );
    }

    #[test]
    fn a_thing_without_locations_has_none() {
        let mut losses = SensorThingsLosses::default();
        for locations in [None, Some(Value::Null), Some(json!([]))] {
            assert_eq!(
                thing_locations(locations, &THING, LineNumber::FIRST, &mut losses).unwrap(),
                ThingLocations::default()
            );
        }
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_location_without_a_readable_id_fails() {
        for locations in [json!([{"encodingType": "application/geo+json"}]), json!(["somewhere"])] {
            let read = thing_locations(Some(locations), &THING, LineNumber::FIRST, &mut SensorThingsLosses::default());
            assert!(read.is_err(), "{read:?}");
        }
    }
}

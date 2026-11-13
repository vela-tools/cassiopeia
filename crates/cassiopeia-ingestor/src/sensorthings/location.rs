use crate::sensorthings::{
    error::{SensorThingsIngestError, StaEntity},
    ingest_losses::LocationEncoding,
    null_members::present,
    served_id::served_id,
};
use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
use serde_json::{Map, Value};

/// The media types a Location's `encodingType` names a `GeoJSON` `location` with: the one RFC 7946
/// registers, and the older one SensorThings servers such as RIVM Samen Meten still send. `GeoJSON` is
/// the only encoding OGC 18-088 defines (section 8.2.2, Table 7).
const GEOJSON_MEDIA_TYPES: [&str; 2] = ["application/geo+json", "application/vnd.geo+json"];

/// Where a Thing is, as its Locations say.
#[derive(Debug, PartialEq)]
pub(super) enum ThingLocation {
    /// The `GeoJSON` geometry of the Thing's Location with the lowest id.
    Geometry(Value),
    /// The Location with the lowest id is encoded in another way than `GeoJSON`.
    Unreadable(LocationEncoding),
    /// The Thing has no Location, or its Location carries no geometry.
    Absent,
}

/// The location of a Thing whose served `Locations` are `locations`, read on `line`.
///
/// The Location with the lowest id is read. A `GeoJSON` `Feature` gives its `geometry` (RFC 7946
/// section 3.2), and any other `GeoJSON` object is the geometry itself.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when a Location has no readable id, since the
/// one read cannot be chosen without it.
pub(super) fn thing_location(locations: Option<Value>, line: LineNumber) -> Result<ThingLocation, SensorThingsIngestError> {
    let Some(Value::Array(locations)) = locations.and_then(present) else {
        return Ok(ThingLocation::Absent);
    };
    let mut lowest: Option<(EntityId, Map<String, Value>)> = None;
    for location in locations {
        let Value::Object(location) = location else {
            return Err(SensorThingsIngestError::IdUnreadable {
                line,
                entity: StaEntity::Location,
            });
        };
        let id = served_id(&location, line, StaEntity::Location)?;
        match &lowest {
            Some((lowest_id, _)) if *lowest_id <= id => {}
            Some(_) | None => lowest = Some((id, location)),
        }
    }
    Ok(lowest.map_or(ThingLocation::Absent, |(_, location)| geometry_of(location)))
}

/// The geometry one Location holds, when it is encoded as `GeoJSON`.
fn geometry_of(mut location: Map<String, Value>) -> ThingLocation {
    match location.get("encodingType") {
        Some(Value::String(encoding)) if is_geojson(encoding) => {}
        Some(Value::String(encoding)) => return ThingLocation::Unreadable(LocationEncoding::Stated(encoding.as_str().into())),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) | None => {
            return ThingLocation::Unreadable(LocationEncoding::Unstated);
        }
    }
    match location.remove("location").and_then(present) {
        Some(Value::Object(mut feature)) if feature.get("type").and_then(Value::as_str) == Some("Feature") => feature
            .remove("geometry")
            .and_then(present)
            .map_or(ThingLocation::Absent, ThingLocation::Geometry),
        Some(geometry) => ThingLocation::Geometry(geometry),
        None => ThingLocation::Absent,
    }
}

/// Whether a media type names `GeoJSON`, compared case-insensitively with its parameters ignored, as
/// media types are (RFC 6838 section 4.2).
fn is_geojson(media_type: &str) -> bool {
    let essence = media_type.split_once(';').map_or(media_type, |(essence, _)| essence).trim();
    GEOJSON_MEDIA_TYPES.iter().any(|geojson| essence.eq_ignore_ascii_case(geojson))
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        ingest_losses::LocationEncoding,
        location::{ThingLocation, thing_location},
    };
    use cassiopeia_sensorthings::snapshot::line_number::LineNumber;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn location_of(locations: Value) -> ThingLocation {
        thing_location(Some(locations), LineNumber::FIRST).unwrap()
    }

    #[test]
    fn the_lowest_location_id_is_used() {
        let locations = json!([
            {"@iot.id": 3, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [3, 3]}},
            {"@iot.id": 2, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [2, 2]}},
        ]);

        assert_eq!(location_of(locations), ThingLocation::Geometry(json!({"type": "Point", "coordinates": [2, 2]})));
    }

    #[test]
    fn a_feature_location_yields_its_geometry() {
        let locations = json!([{
            "@iot.id": "loc-1",
            "encodingType": "application/geo+json",
            "location": {"type": "Feature", "geometry": {"type": "Point", "coordinates": [1, 2]}, "properties": {}},
        }]);

        assert_eq!(location_of(locations), ThingLocation::Geometry(json!({"type": "Point", "coordinates": [1, 2]})));
    }

    #[test]
    fn the_older_geojson_media_type_is_read_in_any_case_and_with_parameters() {
        for encoding in ["application/vnd.geo+json", "Application/GEO+JSON; charset=utf-8"] {
            let locations = json!([{"@iot.id": 1, "encodingType": encoding, "location": {"type": "Point", "coordinates": [5.608, 51.313]}}]);
            assert!(matches!(location_of(locations), ThingLocation::Geometry(_)), "{encoding}");
        }
    }

    #[test]
    fn another_location_encoding_is_unreadable() {
        let locations = json!([{"@iot.id": 1, "encodingType": "text/plain", "location": "here"}]);

        assert_eq!(location_of(locations), ThingLocation::Unreadable(LocationEncoding::Stated("text/plain".into())));
    }

    #[test]
    fn a_location_without_an_encoding_is_unreadable() {
        assert_eq!(
            location_of(json!([{"@iot.id": 1, "location": {"type": "Point", "coordinates": [0, 0]}}])),
            ThingLocation::Unreadable(LocationEncoding::Unstated)
        );
    }

    #[test]
    fn a_thing_without_locations_has_none() {
        assert_eq!(thing_location(None, LineNumber::FIRST).unwrap(), ThingLocation::Absent);
        assert_eq!(location_of(json!([])), ThingLocation::Absent);
        assert_eq!(
            location_of(json!([{"@iot.id": 1, "encodingType": "application/geo+json"}])),
            ThingLocation::Absent
        );
    }

    #[test]
    fn a_location_without_a_readable_id_fails() {
        assert!(thing_location(Some(json!([{"encodingType": "application/geo+json"}])), LineNumber::FIRST).is_err());
        assert!(thing_location(Some(json!(["somewhere"])), LineNumber::FIRST).is_err());
    }
}

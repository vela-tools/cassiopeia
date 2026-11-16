use crate::sensorthings::{
    encoded_geometry::{EncodedGeometry, encoded_geometry},
    error::{SensorThingsIngestError, StaEntity},
    null_members::present,
    served_id::served_id,
};
use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
use serde_json::{Map, Value};

/// The location of a Thing whose served `Locations` are `locations`, read on `line`: the geometry of
/// the Location with the lowest id, read by the rules of [`encoded_geometry`].
///
/// A Thing without Locations has no location.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when a Location has no readable id, since the
/// one read cannot be chosen without it.
pub(super) fn thing_location(locations: Option<Value>, line: LineNumber) -> Result<EncodedGeometry, SensorThingsIngestError> {
    let Some(Value::Array(locations)) = locations.and_then(present) else {
        return Ok(EncodedGeometry::Absent);
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
    Ok(lowest.map_or(EncodedGeometry::Absent, |(_, mut location)| encoded_geometry(&mut location, "location")))
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{encoded_geometry::EncodedGeometry, ingest_losses::GeometryEncoding, location::thing_location};
    use cassiopeia_sensorthings::snapshot::line_number::LineNumber;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn location_of(locations: Value) -> EncodedGeometry {
        thing_location(Some(locations), LineNumber::FIRST).unwrap()
    }

    #[test]
    fn the_lowest_location_id_is_used() {
        let locations = json!([
            {"@iot.id": 3, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [3, 3]}},
            {"@iot.id": 2, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [2, 2]}},
        ]);

        assert_eq!(
            location_of(locations),
            EncodedGeometry::Geometry(json!({"type": "Point", "coordinates": [2, 2]}))
        );
    }

    #[test]
    fn a_feature_location_yields_its_geometry() {
        let locations = json!([{
            "@iot.id": "loc-1",
            "encodingType": "application/geo+json",
            "location": {"type": "Feature", "geometry": {"type": "Point", "coordinates": [1, 2]}, "properties": {}},
        }]);

        assert_eq!(
            location_of(locations),
            EncodedGeometry::Geometry(json!({"type": "Point", "coordinates": [1, 2]}))
        );
    }

    #[test]
    fn the_older_geojson_media_type_is_read_in_any_case_and_with_parameters() {
        for encoding in ["application/vnd.geo+json", "Application/GEO+JSON; charset=utf-8"] {
            let locations = json!([{"@iot.id": 1, "encodingType": encoding, "location": {"type": "Point", "coordinates": [5.608, 51.313]}}]);
            assert!(matches!(location_of(locations), EncodedGeometry::Geometry(_)), "{encoding}");
        }
    }

    #[test]
    fn another_location_encoding_is_unreadable() {
        let locations = json!([{"@iot.id": 1, "encodingType": "text/plain", "location": "here"}]);

        assert_eq!(
            location_of(locations),
            EncodedGeometry::Unreadable(GeometryEncoding::Stated("text/plain".into()))
        );
    }

    #[test]
    fn a_location_without_an_encoding_is_unreadable() {
        assert_eq!(
            location_of(json!([{"@iot.id": 1, "location": {"type": "Point", "coordinates": [0, 0]}}])),
            EncodedGeometry::Unreadable(GeometryEncoding::Unstated)
        );
    }

    #[test]
    fn a_thing_without_locations_has_none() {
        assert_eq!(thing_location(None, LineNumber::FIRST).unwrap(), EncodedGeometry::Absent);
        assert_eq!(location_of(json!([])), EncodedGeometry::Absent);
        assert_eq!(
            location_of(json!([{"@iot.id": 1, "encodingType": "application/geo+json"}])),
            EncodedGeometry::Absent
        );
    }

    #[test]
    fn a_location_without_a_readable_id_fails() {
        assert!(thing_location(Some(json!([{"encodingType": "application/geo+json"}])), LineNumber::FIRST).is_err());
        assert!(thing_location(Some(json!(["somewhere"])), LineNumber::FIRST).is_err());
    }
}

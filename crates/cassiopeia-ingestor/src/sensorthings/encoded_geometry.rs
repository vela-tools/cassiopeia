use crate::sensorthings::{ingest_losses::GeometryEncoding, null_members::present};
use serde_json::{Map, Value};

/// The media types an `encodingType` names `GeoJSON` with: the one RFC 7946 registers, and the older
/// one SensorThings servers such as RIVM Samen Meten still send. `GeoJSON` is the only encoding OGC
/// 18-088 defines for a Location (section 8.2.2, Table 7) and for a `FeatureOfInterest` (section
/// 8.2.8).
const GEOJSON_MEDIA_TYPES: [&str; 2] = ["application/geo+json", "application/vnd.geo+json"];

/// The geometry a served entity encodes, as its `encodingType` says it is encoded.
#[derive(Debug, PartialEq)]
pub(super) enum EncodedGeometry {
    /// The `GeoJSON` geometry the entity holds.
    Geometry(Value),
    /// The entity is encoded in another way than `GeoJSON`.
    Unreadable(GeometryEncoding),
    /// The entity is encoded as `GeoJSON` but holds no geometry.
    Absent,
}

/// The geometry the served `entity` holds in its member `member`, read by its `encodingType`: a
/// Location's `location`, or a `FeatureOfInterest`'s `feature`. The member is taken out of `entity`.
///
/// A `GeoJSON` `Feature` gives its `geometry` (RFC 7946 section 3.2), and any other `GeoJSON` object
/// is the geometry itself.
pub(super) fn encoded_geometry(entity: &mut Map<String, Value>, member: &str) -> EncodedGeometry {
    match entity.get("encodingType") {
        Some(Value::String(encoding)) if is_geojson(encoding) => {}
        Some(Value::String(encoding)) => return EncodedGeometry::Unreadable(GeometryEncoding::Stated(encoding.as_str().into())),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) | None => {
            return EncodedGeometry::Unreadable(GeometryEncoding::Unstated);
        }
    }
    match entity.remove(member).and_then(present) {
        Some(Value::Object(mut feature)) if feature.get("type").and_then(Value::as_str) == Some("Feature") => feature
            .remove("geometry")
            .and_then(present)
            .map_or(EncodedGeometry::Absent, EncodedGeometry::Geometry),
        Some(geometry) => EncodedGeometry::Geometry(geometry),
        None => EncodedGeometry::Absent,
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
        encoded_geometry::{EncodedGeometry, encoded_geometry},
        ingest_losses::GeometryEncoding,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    /// The geometry the served entity `entity` holds in its `feature`.
    fn feature_of(entity: Value) -> (EncodedGeometry, Map<String, Value>) {
        let Value::Object(mut entity) = entity else {
            panic!("expected an object");
        };
        let geometry = encoded_geometry(&mut entity, "feature");
        (geometry, entity)
    }

    #[test]
    fn a_geojson_geometry_is_read_and_taken_out() {
        let (geometry, rest) = feature_of(json!({"encodingType": "application/geo+json", "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]}}));

        assert_eq!(geometry, EncodedGeometry::Geometry(json!({"type": "Point", "coordinates": [5.4797, 51.4433]})));
        assert_eq!(Value::Object(rest), json!({"encodingType": "application/geo+json"}));
    }

    #[test]
    fn a_feature_yields_its_geometry() {
        let (geometry, _) = feature_of(json!({
            "encodingType": "application/geo+json",
            "feature": {"type": "Feature", "geometry": {"type": "Point", "coordinates": [1, 2]}, "properties": {"line": 12}},
        }));

        assert_eq!(geometry, EncodedGeometry::Geometry(json!({"type": "Point", "coordinates": [1, 2]})));
    }

    #[test]
    fn the_older_geojson_media_type_is_read_in_any_case_and_with_parameters() {
        for encoding in ["application/vnd.geo+json", "Application/GEO+JSON; charset=utf-8"] {
            let (geometry, _) = feature_of(json!({"encodingType": encoding, "feature": {"type": "Point", "coordinates": [0, 0]}}));
            assert!(matches!(geometry, EncodedGeometry::Geometry(_)), "{encoding}");
        }
    }

    #[test]
    fn another_encoding_is_unreadable() {
        let (geometry, _) = feature_of(json!({"encodingType": "application/pdf", "feature": "https://h.example/route.pdf"}));

        assert_eq!(geometry, EncodedGeometry::Unreadable(GeometryEncoding::Stated("application/pdf".into())));
    }

    #[test]
    fn an_entity_without_an_encoding_is_unreadable() {
        for entity in [
            json!({"feature": {"type": "Point", "coordinates": [0, 0]}}),
            json!({"encodingType": null, "feature": {"type": "Point", "coordinates": [0, 0]}}),
        ] {
            assert_eq!(feature_of(entity).0, EncodedGeometry::Unreadable(GeometryEncoding::Unstated));
        }
    }

    #[test]
    fn a_geojson_entity_without_a_geometry_has_none() {
        for entity in [
            json!({"encodingType": "application/geo+json"}),
            json!({"encodingType": "application/geo+json", "feature": null}),
            json!({"encodingType": "application/geo+json", "feature": {"type": "Feature", "geometry": null, "properties": {}}}),
        ] {
            assert_eq!(feature_of(entity).0, EncodedGeometry::Absent);
        }
    }
}

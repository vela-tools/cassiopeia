use crate::{detectors::Detector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::media_type;
use serde::{Deserialize, de::IgnoredAny};
use std::str;

/// The `GeoJSON` object types (RFC 7946 section 1.4), used to route a JSON document to the
/// `GeoJSON` branch by its top-level `"type"` member alone.
///
/// The variant identifiers are the exact token strings the spec defines, so serde matches them
/// by name with no rename. Deserialising `type` into this enum fails for any non-`GeoJSON` value,
/// which is how a plain JSON object is told apart from a `GeoJSON` one without materialising either.
#[derive(Deserialize)]
enum GeoJsonType {
    FeatureCollection,
    Feature,
    Point,
    MultiPoint,
    LineString,
    MultiLineString,
    Polygon,
    MultiPolygon,
    GeometryCollection,
}

/// Captures only the top-level `"type"` member of a JSON object and discards everything else.
///
/// Every other field deserialises through serde's ignored-any path, so the whole document is
/// walked for syntactic validity but no value is allocated: no string, array, or map is retained.
#[derive(Deserialize)]
struct TypePeek {
    #[serde(rename = "type")]
    kind: Option<GeoJsonType>,
}

/// Detects the JSON family, distinguishing `GeoJSON` from generic JSON.
///
/// Detection is streaming: the document is parsed for validity without building a
/// `serde_json::Value` tree, so peak memory stays proportional to the input buffer rather than to
/// the far larger node-per-value DOM the buffer would expand into. A top-level object is matched in
/// a single pass through [`TypePeek`], which walks the whole document but keeps only its `"type"`
/// member; a top-level array or scalar (a valid JSON shape [`TypePeek`] cannot represent) is
/// confirmed with one further validity-only pass. `GeoJSON` is routed by its `"type"` member alone
/// (RFC 7946 section 1.4): full structural validation of geometries and features is the ingest
/// stage's concern, not the profiler's.
pub struct JsonDetector;

impl Detector for JsonDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        let content = str::from_utf8(bytes).ok()?;

        match serde_json::from_str::<TypePeek>(content) {
            // A top-level object: GeoJSON when it carries a GeoJSON `type`, plain JSON otherwise.
            Ok(peek) if peek.kind.is_some() => Some(Profile::new(DataFormat::GeoJson, media_type!(APPLICATION / GEO + JSON), 1.0)),
            Ok(_) => Some(Profile::new(DataFormat::Json, media_type!(APPLICATION / JSON), 0.8)),
            // Not an object (a top-level array or scalar), or an object whose `type` is not a
            // GeoJSON token: still generic JSON as long as the whole document is valid.
            Err(_) => serde_json::from_str::<IgnoredAny>(content)
                .ok()
                .map(|_| Profile::new(DataFormat::Json, media_type!(APPLICATION / JSON), 0.8)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::detectors::{Detector, json::JsonDetector};
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn a_feature_collection_is_detected_as_geojson() {
        let profile = JsonDetector.detect(br#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        assert_eq!(*profile.format(), DataFormat::GeoJson);
        assert_eq!(profile.mime_type().to_string(), "application/geo+json");
    }

    #[test]
    fn a_bare_geometry_is_detected_as_geojson_by_its_type_member() {
        let profile = JsonDetector.detect(br#"{"type":"Point","coordinates":[1.0,2.0]}"#).unwrap();
        assert_eq!(*profile.format(), DataFormat::GeoJson);
    }

    #[test]
    fn a_plain_json_object_is_detected_as_generic_json() {
        let profile = JsonDetector.detect(br#"{"name":"value","count":3}"#).unwrap();
        assert_eq!(*profile.format(), DataFormat::Json);
        assert_eq!(profile.mime_type().to_string(), "application/json");
    }

    #[test]
    fn an_object_whose_type_is_not_a_geojson_token_is_generic_json() {
        let profile = JsonDetector.detect(br#"{"type":"sensor-reading","value":42}"#).unwrap();
        assert_eq!(*profile.format(), DataFormat::Json);
    }

    #[test]
    fn a_top_level_array_is_detected_as_generic_json() {
        let profile = JsonDetector.detect(br#"[{"id":1},{"id":2}]"#).unwrap();
        assert_eq!(*profile.format(), DataFormat::Json);
    }

    #[test]
    fn a_top_level_scalar_is_detected_as_generic_json() {
        let profile = JsonDetector.detect(b"42").unwrap();
        assert_eq!(*profile.format(), DataFormat::Json);
    }

    #[test]
    fn invalid_json_is_rejected() {
        assert!(JsonDetector.detect(b"not json at all").is_none());
    }

    #[test]
    fn a_truncated_object_is_rejected() {
        assert!(JsonDetector.detect(br#"{"type":"Feature""#).is_none());
    }
}

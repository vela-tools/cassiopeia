use crate::{content::Content, detectors::Detector, inspectors::sensorthings::SensorThingsInspector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::{MediaType, Name, names::APPLICATION};

/// The media type a snapshot is profiled as: JSON Lines, under the name its tooling registers.
const NDJSON: MediaType<'static> = MediaType::new(APPLICATION, Name::new_unchecked("x-ndjson"));

/// Detects a SensorThings snapshot file by its header line.
///
/// A snapshot is JSON Lines, and its header is a JSON object, so this detector runs before the JSON
/// detector. It only reads the first line, within the first 4 KiB, so a large JSON document is told
/// apart from a snapshot without being parsed here. A SensorThings collection response such as
/// `{"value":[...]}` has no header, so it is still detected as JSON.
pub struct SensorThingsSnapshotDetector;

impl Detector for SensorThingsSnapshotDetector {
    fn detect(&self, content: &Content<'_>) -> Option<Profile> {
        SensorThingsInspector::inspect(content.bytes())
            .ok()
            .map(|_| Profile::new(DataFormat::SensorThings, NDJSON, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        content::Content,
        detectors::{Detector, sensorthings_snapshot::SensorThingsSnapshotDetector},
        profile_bytes,
    };
    use cassiopeia_common::format::DataFormat;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use pretty_assertions::assert_eq;

    const SNAPSHOT: &str = concat!(
        r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://api-samenmeten.rivm.nl/v1.0","version":"v1.0","read":"latest","collectedAt":"2026-11-12T10:00:00Z"}"#,
        "\n",
        r#"{"datastream":{"@iot.id":56776,"name":"ILM_I68-13-pm10"}}"#,
        "\n"
    );

    #[test]
    fn a_snapshot_file_is_detected() {
        let profile = profile_bytes(SNAPSHOT.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(*profile.format(), DataFormat::SensorThings);
        assert_eq!(profile.mime_type().to_string(), "application/x-ndjson");
        assert!((*profile.confidence() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_sensorthings_collection_response_stays_json() {
        let profile = profile_bytes(br#"{"value":[{"@iot.id":1}]}"#, &EncodingHint::NONE).unwrap();

        assert_eq!(*profile.format(), DataFormat::Json);
    }

    #[test]
    fn a_json_lines_file_without_a_header_is_no_snapshot() {
        assert!(SensorThingsSnapshotDetector.detect(&Content::new(b"{\"a\":1}\n{\"a\":2}\n")).is_none());
    }
}

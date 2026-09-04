//! Source-format detection for Cassiopeia's ingest stage.
//!
//! Given a file path or a byte buffer, the profiler runs the content through a
//! series of format detectors and returns the first [`profile::Profile`] that
//! matches. Detection is deliberate rather than heuristic: each detector applies the
//! format-specific checks needed to identify its input, and some detectors inspect a bounded sample.

use crate::{
    detectors::default_detectors,
    error::{DataProfilerError, Result},
    profile::Profile,
};
use cassiopeia_common::error::io::{IoAction, IoError};
use memmap2::Mmap;
use std::{fs::File, path::Path};

pub mod detectors;
pub mod error;
pub mod inspectors;
pub mod metadata;
pub mod profile;

/// Profiles a file at the given path to determine its data format.
///
/// The file is memory-mapped rather than read into a heap buffer, then run through every detector
/// in order, returning the first match. Mapping keeps the profiler's resident memory proportional to
/// the bytes each detector actually touches: a magic-number check faults in only the leading page,
/// and a whole-file scan (JSON validation, a CSV sample) streams through clean read-only pages the
/// kernel can evict under pressure, instead of holding the entire file, which for a multi-gigabyte
/// input is the difference between a few pages and an out-of-memory abort.
///
/// # Arguments
/// * `path` - The path to the file to profile.
///
/// # Errors
/// Returns [`DataProfilerError::NotFound`] if the path does not exist,
/// [`DataProfilerError::Io`] if the file cannot be opened or mapped, or
/// [`DataProfilerError::UnknownFormat`] if no detector recognises the content (an empty file
/// included, as it can match no format).
pub fn profile_path(path: impl AsRef<Path>) -> Result<Profile> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(DataProfilerError::NotFound(path.to_path_buf()));
    }

    let file = File::open(path).map_err(|source| IoError::FileOperation {
        source,
        path: path.to_path_buf(),
        action: IoAction::Open,
    })?;
    let length = file
        .metadata()
        .map_err(|source| IoError::FileOperation {
            source,
            path: path.to_path_buf(),
            action: IoAction::Read,
        })?
        .len();
    // A zero-length file cannot be mapped (a zero-length mapping is an error on most platforms) and
    // matches no detector regardless, so it is an unknown format rather than an I/O failure.
    if length == 0 {
        return Err(DataProfilerError::UnknownFormat(path.to_path_buf()));
    }

    // SAFETY: the mapping is read-only and lives only for the duration of detection, during which
    // Cassiopeia never writes to the path. The residual hazard is another process truncating the
    // file underneath the mapping; that is the accepted trade-off for sniffing large inputs without
    // loading them, the same one made by ripgrep and fd.
    let mapping = unsafe { Mmap::map(&file) }.map_err(|source| IoError::FileOperation {
        source,
        path: path.to_path_buf(),
        action: IoAction::Read,
    })?;

    default_detectors()
        .iter()
        .find_map(|detector| detector.detect(&mapping))
        .ok_or_else(|| DataProfilerError::UnknownFormat(path.to_path_buf()))
}

/// Profiles raw bytes to determine their data format.
///
/// Similar to `profile_path`, but works with data already in memory.
///
/// # Arguments
/// * `bytes` - The data to profile.
///
/// # Errors
/// Returns [`DataProfilerError::UnknownBytesFormat`] if no detector recognises the content.
pub fn profile_bytes(bytes: &[u8]) -> Result<Profile> {
    default_detectors()
        .iter()
        .find_map(|detector| detector.detect(bytes))
        .ok_or(DataProfilerError::UnknownBytesFormat)
}

#[cfg(test)]
mod tests {
    use crate::{error::DataProfilerError, metadata::FormatMetadata, profile_bytes};
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn kml_is_detected_by_its_root_element_and_ogc_namespace() {
        let bytes = br#"<?xml version="1.0"?><kml xmlns="http://www.opengis.net/kml/2.2"><Document/></kml>"#;
        let profile = profile_bytes(bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::Kml);
    }

    #[test]
    fn a_feature_collection_is_detected_as_geojson_not_plain_json() {
        let bytes = br#"{"type":"FeatureCollection","features":[]}"#;
        let profile = profile_bytes(bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::GeoJson);
    }

    #[test]
    fn a_generic_xml_document_falls_through_to_xml_without_metadata() {
        let profile = profile_bytes(b"<?xml version=\"1.0\"?><data><metData/></data>").unwrap();
        assert_eq!(*profile.format(), DataFormat::Xml);
        assert!(profile.metadata().is_none());
    }

    #[test]
    fn a_kml_document_is_detected_as_kml_not_generic_xml() {
        let bytes = br#"<?xml version="1.0"?><kml xmlns="http://www.opengis.net/kml/2.2"><Document/></kml>"#;
        assert_eq!(*profile_bytes(bytes).unwrap().format(), DataFormat::Kml);
    }

    #[test]
    fn a_plain_json_object_falls_through_to_generic_json() {
        let bytes = br#"{"name":"value","count":3}"#;
        let profile = profile_bytes(bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::Json);
    }

    #[test]
    fn a_header_row_with_records_is_detected_as_csv() {
        let bytes = b"id,name,value\n1,alpha,10\n2,beta,20\n";
        let profile = profile_bytes(bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::Csv);
        assert_eq!(profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap().dialect().delimiter, b',');
    }

    #[test]
    fn a_semicolon_csv_carries_its_detected_dialect() {
        let profile = profile_bytes(b"id;name;value\n1;alpha;10\n2;beta;20\n").unwrap();

        assert_eq!(*profile.format(), DataFormat::Csv);
        assert_eq!(profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap().dialect().delimiter, b';');
    }

    #[test]
    fn content_matching_no_detector_is_reported_as_unknown() {
        let bytes = &[0xffu8, 0x00, 0xfe];
        assert!(matches!(profile_bytes(bytes), Err(DataProfilerError::UnknownBytesFormat)));
    }
}

use crate::{content::Content, detectors::Detector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::{
    MediaType,
    Name,
    names::{APPLICATION, GEOPACKAGE},
};

/// The IANA-registered media type of a `GeoPackage`, `application/geopackage+sqlite3`.
const GEOPACKAGE_MEDIA_TYPE: MediaType<'static> = MediaType::from_parts(APPLICATION, GEOPACKAGE, Some(Name::new_unchecked("sqlite3")), &[]);

/// The 16-octet header string every SQLite 3 database file opens with.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Where the SQLite header stores the big-endian `application_id` pragma.
const APPLICATION_ID_OFFSET: usize = 68;

/// The `application_id` values a `GeoPackage` declares: `GPKG` since `GeoPackage` 1.2, and the legacy
/// `GP10` and `GP11` of `GeoPackage` 1.0 and 1.1.
const GEOPACKAGE_APPLICATION_IDS: [&[u8; 4]; 3] = [b"GPKG", b"GP10", b"GP11"];

/// Detects an OGC `GeoPackage` from its SQLite file header.
///
/// A `GeoPackage` is a SQLite 3 database (OGC 12-128r19 Requirement 1) that declares itself through the
/// `application_id` the SQLite header stores at offset 68 (Requirement 2). Both checks run on the
/// leading bytes alone, so a plain SQLite database that is not a `GeoPackage` is not claimed. The tables
/// a file holds are discovered by the ingestor, so no metadata is attached.
pub struct GeoPackageDetector;

impl Detector for GeoPackageDetector {
    fn detect(&self, content: &Content<'_>) -> Option<Profile> {
        let bytes = content.bytes();
        let application_id = bytes.get(APPLICATION_ID_OFFSET..APPLICATION_ID_OFFSET + 4)?;
        let is_geopackage = bytes.starts_with(SQLITE_HEADER) && GEOPACKAGE_APPLICATION_IDS.iter().any(|id| id.as_slice() == application_id);
        is_geopackage.then(|| Profile::new(DataFormat::GeoPackage, GEOPACKAGE_MEDIA_TYPE, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        content::Content,
        detectors::{Detector, geopackage::GeoPackageDetector},
    };
    use cassiopeia_common::format::DataFormat;
    use pretty_assertions::assert_eq;

    /// A 100-octet SQLite header declaring `application_id`.
    fn sqlite_header(application_id: [u8; 4]) -> Vec<u8> {
        let mut bytes = b"SQLite format 3\0".to_vec();
        bytes.resize(100, 0);
        bytes[68..72].copy_from_slice(&application_id);
        bytes
    }

    #[test]
    fn a_sqlite_header_declaring_gpkg_is_detected_as_a_geopackage() {
        let profile = GeoPackageDetector.detect(&Content::new(&sqlite_header(*b"GPKG"))).unwrap();
        assert_eq!(*profile.format(), DataFormat::GeoPackage);
        assert_eq!(profile.mime_type().to_string(), "application/geopackage+sqlite3");
    }

    #[test]
    fn the_legacy_gp10_and_gp11_application_ids_are_detected_as_a_geopackage() {
        for id in [b"GP10", b"GP11"] {
            let profile = GeoPackageDetector.detect(&Content::new(&sqlite_header(*id))).unwrap();
            assert_eq!(*profile.format(), DataFormat::GeoPackage);
        }
    }

    #[test]
    fn a_plain_sqlite_database_is_not_a_geopackage() {
        assert!(GeoPackageDetector.detect(&Content::new(&sqlite_header([0, 0, 0, 0]))).is_none());
    }

    #[test]
    fn a_gpkg_application_id_without_the_sqlite_header_is_not_a_geopackage() {
        let mut bytes = sqlite_header(*b"GPKG");
        bytes[..16].copy_from_slice(b"Not SQLite at al");
        assert!(GeoPackageDetector.detect(&Content::new(&bytes)).is_none());
    }

    #[test]
    fn a_header_truncated_before_the_application_id_is_not_a_geopackage() {
        assert!(GeoPackageDetector.detect(&Content::new(&sqlite_header(*b"GPKG")[..70])).is_none());
    }

    #[test]
    fn arbitrary_content_is_not_a_geopackage() {
        assert!(GeoPackageDetector.detect(&Content::new(b"id,name\n1,alpha\n")).is_none());
        assert!(GeoPackageDetector.detect(&Content::new(b"")).is_none());
    }
}

use crate::{detectors::Detector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::{MediaType, Name, names::APPLICATION};
use std::io::Cursor;
use zip::ZipArchive;

/// The unregistered but conventional media type for an ESRI Shapefile.
const SHAPEFILE_MEDIA_TYPE: MediaType<'static> = MediaType::new(APPLICATION, Name::new_unchecked("x-shapefile"));

/// The four-octet big-endian file code (9994) every `.shp` main file opens with.
const SHP_FILE_CODE: [u8; 4] = [0x00, 0x00, 0x27, 0x0A];

/// Detects an ESRI Shapefile in either of its two carriers: a bare `.shp` main file or a `.zip`
/// bundling the companion set.
///
/// A bare `.shp` is recognised by its four-octet file code (9994) in octets 1-4 of the main-file
/// header (ESRI Shapefile Technical Description, "The Main File Header"). A `.zip` bundle is recognised
/// when any archive member has a `.shp` extension; a single bundle may carry several `.shp` layers. The
/// carrier is not distinguished here: both profile as [`DataFormat::Shapefile`] and the ingestor
/// branches on the leading bytes. Layer discovery is likewise deferred to the ingestor, so no metadata
/// is attached.
pub struct ShapefileDetector;

impl Detector for ShapefileDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        if bytes.starts_with(&SHP_FILE_CODE) || zip_holds_a_shp(bytes) {
            return Some(Profile::new(DataFormat::Shapefile, SHAPEFILE_MEDIA_TYPE, 1.0));
        }
        None
    }
}

/// Reports whether `bytes` is a zip archive holding at least one `.shp` member.
///
/// This member scan looks only for a `.shp` entry, so a KMZ archive (whose members are `.kml`) is
/// never mistaken for a shapefile bundle and vice versa.
fn zip_holds_a_shp(bytes: &[u8]) -> bool {
    let Ok(mut archive) = ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    (0..archive.len()).any(|index| {
        // A single unreadable entry must not abort the scan; skip it and keep looking.
        archive.by_index(index).is_ok_and(|entry| entry.name().to_ascii_lowercase().ends_with(".shp"))
    })
}

#[cfg(test)]
mod tests {
    use crate::detectors::{Detector, shapefile::ShapefileDetector};
    use cassiopeia_common::format::DataFormat;
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn zip_with(names: &[&str]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut archive = ZipWriter::new(&mut cursor);
            for name in names {
                archive.start_file(*name, SimpleFileOptions::default()).unwrap();
                archive.write_all(b"body").unwrap();
            }
            archive.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn the_shp_file_code_is_detected_as_a_shapefile() {
        let mut bytes = vec![0x00, 0x00, 0x27, 0x0A];
        bytes.extend_from_slice(&[0u8; 96]);
        let profile = ShapefileDetector.detect(&bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::Shapefile);
        assert_eq!(profile.mime_type().to_string(), "application/x-shapefile");
    }

    #[test]
    fn a_zip_holding_a_shp_member_is_detected_as_a_shapefile() {
        let bytes = zip_with(&["roads.shp", "roads.dbf", "roads.shx"]);
        let profile = ShapefileDetector.detect(&bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::Shapefile);
    }

    #[test]
    fn a_zip_without_a_shp_member_is_not_a_shapefile() {
        assert!(ShapefileDetector.detect(&zip_with(&["doc.kml"])).is_none());
        assert!(ShapefileDetector.detect(&zip_with(&["xl/workbook.xml"])).is_none());
    }

    #[test]
    fn arbitrary_text_is_not_a_shapefile() {
        assert!(ShapefileDetector.detect(b"id,name\n1,alpha\n").is_none());
        assert!(ShapefileDetector.detect(br#"{"type":"FeatureCollection"}"#).is_none());
        assert!(ShapefileDetector.detect(b"").is_none());
    }
}

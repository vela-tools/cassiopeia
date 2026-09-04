use crate::{detectors::Detector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::media_type;
use std::{
    io::{Cursor, Read},
    str,
};
use zip::ZipArchive;

/// Detects KML and KMZ by their KML root element and OGC namespace.
pub struct KmlDetector;

impl Detector for KmlDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        if is_kml(bytes) {
            return Some(Profile::new(DataFormat::Kml, media_type!(APPLICATION / vnd::GOOGLE_EARTH_KML + XML), 1.0));
        }

        let mut archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
        for index in 0..archive.len() {
            // A single unreadable entry must not abort the whole scan; skip it and keep looking.
            let Ok(mut entry) = archive.by_index(index) else {
                continue;
            };
            if !entry.name().to_ascii_lowercase().ends_with(".kml") {
                continue;
            }

            let mut kml = Vec::new();
            if entry.read_to_end(&mut kml).is_err() {
                continue;
            }
            if is_kml(&kml) {
                return Some(Profile::new(DataFormat::Kmz, media_type!(APPLICATION / vnd::GOOGLE_EARTH_KMZ), 1.0));
            }
        }

        None
    }
}

fn is_kml(bytes: &[u8]) -> bool {
    let Some(content) = str::from_utf8(bytes).ok() else {
        return false;
    };

    // The `<kml` root element paired with the OGC namespace uniquely marks KML.
    content.contains("<kml") && content.contains("opengis.net/kml")
}

#[cfg(test)]
mod tests {
    use crate::detectors::{Detector, kml::KmlDetector};
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn kml_is_detected_by_its_root_element_and_ogc_namespace() {
        let bytes = br#"<?xml version="1.0"?><kml xmlns="http://www.opengis.net/kml/2.2"><Document/></kml>"#;
        let profile = KmlDetector.detect(bytes).unwrap();
        assert_eq!(*profile.format(), DataFormat::Kml);
        assert_eq!(profile.mime_type().to_string(), "application/vnd.google-earth.kml+xml");
    }

    #[test]
    fn xml_without_the_kml_namespace_is_rejected() {
        assert!(KmlDetector.detect(br"<kml>no namespace</kml>").is_none());
    }

    #[test]
    fn kmz_is_detected_by_a_valid_kml_entry() {
        use std::io::{Cursor, Write};
        use zip::{ZipWriter, write::SimpleFileOptions};

        let mut bytes = Cursor::new(Vec::new());
        {
            let mut archive = ZipWriter::new(&mut bytes);
            archive.start_file("doc.kml", SimpleFileOptions::default()).unwrap();
            archive.write_all(br#"<kml xmlns="http://www.opengis.net/kml/2.2"><Document/></kml>"#).unwrap();
            archive.finish().unwrap();
        }

        let profile = KmlDetector.detect(bytes.get_ref()).unwrap();
        assert_eq!(*profile.format(), DataFormat::Kmz);
        assert_eq!(profile.mime_type().to_string(), "application/vnd.google-earth.kmz");
    }
}

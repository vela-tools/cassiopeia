use crate::{detectors::Detector, inspectors::grib::GribInspector, metadata::FormatMetadata, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::{MediaType, Name, names::APPLICATION};

/// The unregistered but conventional media type for GRIB gridded binary data.
const GRIB_MEDIA_TYPE: MediaType<'static> = MediaType::new(APPLICATION, Name::new_unchecked("x-grib"));

/// Detects GRIB by its four-octet `GRIB` magic at the start of the file.
///
/// Detection is edition-agnostic: both GRIB1 and GRIB2 begin with the same magic, so both profile as
/// [`DataFormat::Grib`]. The edition itself is then read by [`GribInspector`] and attached as
/// [`FormatMetadata::Grib`], so the ingestor dispatches on it without re-reading the header. A truncated
/// header or an unrecognised edition octet attaches no metadata; the ingestor then falls back to reading
/// the edition from the file.
pub struct GribDetector;

impl Detector for GribDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        // WMO GRIB (both editions): "GRIB" in octets 1-4 of the Indicator Section.
        if !bytes.starts_with(b"GRIB") {
            return None;
        }
        let profile = Profile::new(DataFormat::Grib, GRIB_MEDIA_TYPE, 1.0);
        Some(match GribInspector::inspect(bytes) {
            Some(metadata) => profile.with_metadata(FormatMetadata::Grib(metadata)),
            None => profile,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        detectors::{Detector, grib::GribDetector},
        metadata::{FormatMetadata, GribEdition},
    };
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn grib2_magic_is_detected_as_grib_with_edition_metadata() {
        let profile = GribDetector.detect(b"GRIB\x00\x00\x00\x02\x00\x00\x00\x00").unwrap();
        assert_eq!(*profile.format(), DataFormat::Grib);
        assert_eq!(profile.mime_type().to_string(), "application/x-grib");
        let edition = profile.metadata().as_ref().and_then(FormatMetadata::as_grib).unwrap().edition();
        assert_eq!(edition, GribEdition::V2);
    }

    #[test]
    fn grib1_magic_is_detected_as_grib_with_edition_metadata() {
        let profile = GribDetector.detect(b"GRIB\x00\x00\x1c\x01").unwrap();
        assert_eq!(*profile.format(), DataFormat::Grib);
        let edition = profile.metadata().as_ref().and_then(FormatMetadata::as_grib).unwrap().edition();
        assert_eq!(edition, GribEdition::V1);
    }

    #[test]
    fn grib_magic_without_a_readable_edition_octet_carries_no_metadata() {
        // Magic present but the buffer stops before octet 8: the edition cannot be inspected.
        let profile = GribDetector.detect(b"GRIB\x00\x00").unwrap();
        assert_eq!(*profile.format(), DataFormat::Grib);
        assert!(profile.metadata().is_none());
    }

    #[test]
    fn non_grib_bytes_are_rejected() {
        assert!(GribDetector.detect(br#"{"type":"FeatureCollection"}"#).is_none());
        assert!(GribDetector.detect(b"id,name\n1,alpha\n").is_none());
        assert!(GribDetector.detect(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(GribDetector.detect(b"").is_none());
    }
}

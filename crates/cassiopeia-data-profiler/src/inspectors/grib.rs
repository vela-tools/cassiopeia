use crate::metadata::{GribEdition, GribMetadata};

/// The structural inspector for GRIB gridded binary data.
///
/// GRIB1 and GRIB2 share the `GRIB` magic but are decoded by different code paths, so the one thing a
/// profiler must recover before dispatch is the edition. This inspector reads it from the Indicator
/// Section and runs whichever way the format was decided, as part of auto-detection and for a declared
/// GRIB alike, so the ingestor never has to re-read the header to choose a decoder.
pub struct GribInspector;

impl GribInspector {
    /// Reads the GRIB edition from octet 8 (index 7) of the Indicator Section.
    ///
    /// Both editions place the edition number at the same offset, so a single byte read after the
    /// four-octet magic suffices. Bytes that do not start with the `GRIB` magic, a header truncated
    /// before octet 8, or an edition other than 1 or 2 all yield `None`; the caller then defers to the
    /// ingestor's own read of the header, which is the authoritative declaration of the format.
    #[must_use]
    pub fn inspect(bytes: &[u8]) -> Option<GribMetadata> {
        if !bytes.starts_with(b"GRIB") {
            return None;
        }
        match bytes.get(7) {
            Some(1) => Some(GribMetadata::new(GribEdition::V1)),
            Some(2) => Some(GribMetadata::new(GribEdition::V2)),
            Some(_) | None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{inspectors::grib::GribInspector, metadata::GribEdition};

    #[test]
    fn grib2_edition_is_read_from_octet_eight() {
        let metadata = GribInspector::inspect(b"GRIB\x00\x00\x00\x02\x00\x00\x00\x00").unwrap();
        assert_eq!(metadata.edition(), GribEdition::V2);
    }

    #[test]
    fn grib1_edition_is_read_from_octet_eight() {
        let metadata = GribInspector::inspect(b"GRIB\x00\x00\x1c\x01").unwrap();
        assert_eq!(metadata.edition(), GribEdition::V1);
    }

    #[test]
    fn a_header_truncated_before_the_edition_octet_yields_no_metadata() {
        assert!(GribInspector::inspect(b"GRIB\x00\x00").is_none());
    }

    #[test]
    fn an_unrecognised_edition_octet_yields_no_metadata() {
        assert!(GribInspector::inspect(b"GRIB\x00\x00\x00\x09").is_none());
    }

    #[test]
    fn bytes_without_the_grib_magic_yield_no_metadata() {
        assert!(GribInspector::inspect(b"id,name\n1,alpha\n").is_none());
    }
}

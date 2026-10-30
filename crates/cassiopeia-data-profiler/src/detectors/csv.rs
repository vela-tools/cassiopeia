use crate::{
    detectors::Detector,
    inspectors::csv::{CsvInspector, dialect::Dialect},
    metadata::{CsvMetadata, FormatMetadata},
    profile::Profile,
};
use cassiopeia_common::format::DataFormat;
use mediatype::media_type;

/// Detects delimited text by parsing a header row and a small sample of records.
///
/// Deliberately the last detector in the chain: it is the most permissive, so
/// more specific formats are matched first.
pub struct CsvDetector;

impl Detector for CsvDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        let dialect = CsvInspector::new().inspect(bytes).ok()?;
        if !dialect.header_rows.is_present() || !Self::has_header_and_record(bytes, &dialect) {
            return None;
        }
        Some(Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 0.9).with_metadata(FormatMetadata::Csv(CsvMetadata::new(dialect))))
    }
}

impl CsvDetector {
    fn has_header_and_record(bytes: &[u8], dialect: &Dialect) -> bool {
        // Decoded as the ingestor decodes the file, so a non-UTF-8 header does not fail the check.
        let decoded = dialect.character_encoding().decode(bytes).0;
        let Ok(mut reader) = dialect.reader(decoded.as_bytes()) else {
            return false;
        };
        let Ok(headers) = reader.headers() else {
            return false;
        };
        !headers.is_empty() && reader.records().next().is_some_and(|record| record.is_ok())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        detectors::{Detector, csv::CsvDetector},
        inspectors::csv::{header_rows::HeaderRows, preamble::Preamble},
        metadata::FormatMetadata,
    };
    use cassiopeia_common::format::DataFormat;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_header_row_with_records_is_detected_as_csv() {
        let profile = CsvDetector.detect(b"id,name,value\n1,alpha,10\n2,beta,20\n").unwrap();
        assert_eq!(*profile.format(), DataFormat::Csv);
        assert_eq!(profile.mime_type().to_string(), "text/csv");
        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();
        assert_eq!(metadata.dialect().delimiter, b',');
    }

    #[test]
    fn a_header_only_buffer_with_no_records_is_rejected() {
        assert!(CsvDetector.detect(b"id,name,value").is_none());
    }

    #[test]
    fn an_empty_buffer_is_rejected() {
        assert!(CsvDetector.detect(b"").is_none());
    }

    #[test]
    fn a_windows_1252_stacked_header_is_detected_as_csv() {
        let mut bytes = b"Lumes;v2.10\n;NO2;O3\n;HMW;HMW\n;\xb5g/m\xb3;\xb5g/m\xb3\n".to_vec();
        bytes.extend_from_slice(b"STEF;18,48;50,74\nTAB;NE;10,37\nKE;15,17;8,48\n");
        let profile = CsvDetector.detect(&bytes).unwrap();
        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();

        assert_eq!(metadata.dialect().header_rows, HeaderRows::new(3));
    }

    #[test]
    fn a_titled_table_is_detected_as_csv_with_its_preamble() {
        let profile = CsvDetector.detect(b"Lumes;v2.10\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n").unwrap();
        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();

        assert_eq!(metadata.dialect().preamble, Preamble::new(1));
        assert_eq!(metadata.dialect().header_rows, HeaderRows::ONE);
    }
}

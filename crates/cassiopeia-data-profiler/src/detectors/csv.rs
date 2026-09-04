use crate::{
    detectors::Detector,
    inspectors::csv::{CsvInspector, dialect::Dialect},
    metadata::{CsvMetadata, FormatMetadata},
    profile::Profile,
};
use cassiopeia_common::format::DataFormat;
use csv::ReaderBuilder;
use mediatype::media_type;

/// Detects delimited text by parsing a header row and a small sample of records.
///
/// Deliberately the last detector in the chain: it is the most permissive, so
/// more specific formats are matched first.
pub struct CsvDetector;

impl Detector for CsvDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        let dialect = CsvInspector::new().inspect(bytes).ok()?;
        if !dialect.has_headers || !Self::has_header_and_record(bytes, &dialect) {
            return None;
        }
        Some(Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 0.9).with_metadata(FormatMetadata::Csv(CsvMetadata::new(dialect))))
    }
}

impl CsvDetector {
    fn has_header_and_record(bytes: &[u8], dialect: &Dialect) -> bool {
        let mut builder = ReaderBuilder::new();
        builder
            .delimiter(dialect.delimiter)
            .has_headers(dialect.has_headers)
            .terminator(dialect.terminator.as_csv());

        if let Some(quote) = dialect.quote_char {
            builder.quote(quote);
        } else {
            builder.quoting(false);
        }
        builder.escape(dialect.escape);

        let mut reader = builder.from_reader(bytes);
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
        metadata::FormatMetadata,
    };
    use cassiopeia_common::format::DataFormat;

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
}

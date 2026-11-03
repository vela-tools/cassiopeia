use crate::{
    content::Content,
    detectors::Detector,
    inspectors::csv::CsvInspector,
    metadata::{CsvMetadata, FormatMetadata},
    profile::Profile,
};
use cassiopeia_common::format::DataFormat;
use cassiopeia_csv_dialect::{dialect::Dialect, misshapen_row::MisshapenRow, statement::DialectStatement};
use mediatype::media_type;

/// Detects delimited text by parsing a header row and a small sample of records.
///
/// Deliberately the last detector in the chain: it is the most permissive, so
/// more specific formats are matched first.
pub struct CsvDetector;

impl Detector for CsvDetector {
    fn detect(&self, content: &Content<'_>) -> Option<Profile> {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(content).ok()?;
        if !dialect.header_rows.is_present() || !Self::has_header_and_record(content.bytes(), &dialect) {
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
        // The ingestor skips a misshapen row, so the first record that is not one decides.
        !headers.is_empty()
            && reader
                .records()
                .find(|record| {
                    record
                        .as_ref()
                        .map_or_else(|error| MisshapenRow::of(error, dialect.preamble).is_none(), |_| true)
                })
                .is_some_and(|record| record.is_ok())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        content::Content,
        detectors::{Detector, csv::CsvDetector},
        encoding_hint::EncodingHint,
        inspectors::csv::CsvInspector,
        metadata::FormatMetadata,
    };
    use cassiopeia_common::format::DataFormat;
    use cassiopeia_csv_dialect::{dialect_byte::DialectByte, header_rows::HeaderRows, preamble::Preamble, statement::DialectStatement};
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn a_file_with_a_footer_note_and_a_ragged_row_is_detected_as_csv() {
        let profile = CsvDetector
            .detect(&Content::new(b"id;name;value\n1;a;10\n2;b\n3;c;30\n4;d;40\nSource: City statistics office\n"))
            .unwrap();

        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();
        assert_eq!(metadata.dialect().delimiter, DialectByte::SEMICOLON);
        assert_eq!(metadata.dialect().header_rows, HeaderRows::ONE);
        assert_eq!(metadata.dialect().preamble, Preamble::NONE);
    }

    #[test]
    fn a_first_data_row_that_is_misshapen_does_not_hide_the_rows_below_it() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(b"id;name;value\n1;a;10\n2;b;20\n3;c;30\n"))
            .unwrap();

        assert!(CsvDetector::has_header_and_record(b"id;name;value\n1;a\n2;b;20\n", &dialect));
    }

    #[test]
    fn a_header_row_with_records_is_detected_as_csv() {
        let profile = CsvDetector.detect(&Content::new(b"id,name,value\n1,alpha,10\n2,beta,20\n")).unwrap();
        assert_eq!(*profile.format(), DataFormat::Csv);
        assert_eq!(profile.mime_type().to_string(), "text/csv");
        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();
        assert_eq!(metadata.dialect().delimiter, DialectByte::COMMA);
    }

    #[test]
    fn a_header_only_buffer_with_no_records_is_rejected() {
        assert!(CsvDetector.detect(&Content::new(b"id,name,value")).is_none());
    }

    #[test]
    fn an_empty_buffer_is_rejected() {
        assert!(CsvDetector.detect(&Content::new(b"")).is_none());
    }

    #[test]
    fn a_windows_1252_stacked_header_is_detected_as_csv() {
        let mut bytes = b"Lumes;v2.10\n;NO2;O3\n;HMW;HMW\n;\xb5g/m\xb3;\xb5g/m\xb3\n".to_vec();
        bytes.extend_from_slice(b"STEF;18,48;50,74\nTAB;NE;10,37\nKE;15,17;8,48\n");
        let profile = CsvDetector.detect(&Content::new(&bytes)).unwrap();
        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();

        assert_eq!(metadata.dialect().header_rows, HeaderRows::new(3));
    }

    #[test]
    fn an_austrian_source_reads_a_windows_1252_stacked_header_with_its_unit_symbols() {
        // Vienna's air-quality feed: windows-1252 unit symbols that content alone reads as ISO-8859-2
        // letters (`ľg/mł`). The domain the feed is served from settles the encoding.
        let mut bytes = b"Lumes;v2.10\n;NO2;O3\n;HMW;HMW\n;\xb5g/m\xb3;\xb5g/m\xb3\n".to_vec();
        bytes.extend_from_slice(b"STEF;18,48;50,74\nTAB;NE;10,37\nKE;15,17;8,48\n");
        let hint = EncodingHint::for_remote_source(&Url::parse("https://www.wien.gv.at/ma22-lgb/umweltgut/lumesakt-v2.csv").unwrap(), None);
        let profile = CsvDetector.detect(&Content::hinted(&bytes, &hint)).unwrap();
        let dialect = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap().dialect();
        let decoded = dialect.character_encoding().decode(&bytes).0;
        let mut reader = dialect.reader(decoded.as_bytes()).unwrap();

        assert_eq!(dialect.encoding.to_string(), "windows-1252");
        assert_eq!(reader.headers().unwrap(), vec!["", "NO2 HMW µg/m³", "O3 HMW µg/m³"]);
    }

    #[test]
    fn a_titled_table_is_detected_as_csv_with_its_preamble() {
        let profile = CsvDetector
            .detect(&Content::new(b"Lumes;v2.10\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n"))
            .unwrap();
        let metadata = profile.metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();

        assert_eq!(metadata.dialect().preamble, Preamble::new(1));
        assert_eq!(metadata.dialect().header_rows, HeaderRows::ONE);
    }
}

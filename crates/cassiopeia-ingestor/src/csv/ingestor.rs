use crate::{csv::error::CsvIngestError, error::IngestorError, ingestor::Ingestor};
use ::csv::{Reader, StringRecord};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_data_profiler::{inspectors::csv::dialect::Dialect, metadata::FormatMetadata};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use encoding_rs::UTF_8;
use encoding_rs_io::{DecodeReaderBytes, DecodeReaderBytesBuilder};
use serde_json::{Map, Number, Value};
use std::{fs::File, io::BufReader, mem};
use tracing::{info, warn};

/// A CSV reader over a file decoded to UTF-8 with its profiled encoding.
type DecodedFileReader = Reader<BufReader<DecodeReaderBytes<File, Vec<u8>>>>;

/// Ingestor for CSV data.
///
/// Uses the profiled CSV dialect when one is available, then parses records with
/// smart value inference.
pub struct CsvIngestor {
    reader: Option<DecodedFileReader>,
    /// The record key of each column, from the header; `None` for a headerless source, whose
    /// columns are keyed by position.
    column_keys: Option<Vec<String>>,
    batch_size: usize,
}

impl CsvIngestor {
    /// Creates a CSV ingestor from a profiled payload and its detected dialect metadata.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file, the file cannot be opened, or the
    /// CSV reader cannot be built.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<CsvIngestor, IngestorError> {
        let dialect = payload
            .profile()
            .metadata()
            .as_ref()
            .and_then(FormatMetadata::as_csv)
            .map(|metadata| metadata.dialect().clone());

        let path = match payload.into_payload() {
            CollectedPayload::File(f) => f.into_path(),
            CollectedPayload::Bytes(_) => return Err(CsvIngestError::RequiresFile.into()),
        };

        let encoding = dialect.as_ref().map_or(UTF_8, Dialect::character_encoding);

        // Decode the file to UTF-8 with the profiled encoding, so a non-UTF-8 source (Windows-1252,
        // Latin-1, ...) is read correctly instead of failing on its first non-UTF-8 byte.
        let file = File::open(&path).map_err(|source| CsvIngestError::Open {
            source,
            // The error owns the path after this borrow ends.
            path: path.clone(),
        })?;
        let decoded = BufReader::new(DecodeReaderBytesBuilder::new().encoding(Some(encoding)).build(file));

        // The profiled dialect opens the file exactly as the inspector parsed its sample, preamble
        // skip and stacked-header join included, so the header read below is the one the inspector
        // detected.
        let mut reader = match &dialect {
            Some(dialect) => {
                info!("Using profiled CSV dialect: {:#?}", dialect);
                dialect.reader(decoded).map_err(|source| CsvIngestError::TableStart {
                    source,
                    // The error owns the path after this borrow ends.
                    path: path.clone(),
                })?
            }
            None => Reader::from_reader(decoded),
        };

        // A headerless source has no names to key its columns by; its records are keyed by position
        // instead, so the first row is not consumed as a header and nothing is lost.
        let column_keys = if dialect.as_ref().is_none_or(|dialect| dialect.header_rows.is_present()) {
            let keys = Self::column_keys(reader.headers().map_err(CsvIngestError::Reader)?);
            info!("Column keys: {:#?}", keys);
            Some(keys)
        } else {
            info!("Headerless CSV: columns keyed by position");
            None
        };

        Ok(CsvIngestor {
            reader: Some(reader),
            column_keys,
            batch_size,
        })
    }

    /// Normalizes a header for use as a record key.
    ///
    /// A quoted header field may hold an embedded newline (RFC 4180 clause 2.6); folding it to a
    /// space keeps the key on one line. The header is otherwise preserved verbatim: its Unicode
    /// and punctuation are addressable directly from a mapping, so no transliteration is applied.
    fn clean_header(header: &str) -> String {
        header.replace('\n', " ")
    }

    /// The record key of each column: its cleaned header, or its zero-based position when the header
    /// is blank, as for a headerless source. A stacked header's key column often has no label at all.
    fn column_keys(headers: &StringRecord) -> Vec<String> {
        headers
            .iter()
            .enumerate()
            .map(|(index, header)| {
                if header.trim().is_empty() {
                    index.to_string()
                } else {
                    Self::clean_header(header)
                }
            })
            .collect()
    }

    /// Smart value inference for a CSV field.
    ///
    /// Leading-zero numbers are kept as strings (identifiers such as postal
    /// codes), otherwise integers, floats, and booleans are parsed out.
    fn infer_value(field: &str) -> Value {
        let field = field.trim();
        if field.is_empty() {
            return Value::Null;
        }

        // `\N` is the null token emitted by SQL/`COPY`-style CSV dumps (OpenFlights among them); it
        // is an absent value, not the literal two-character string.
        if field == r"\N" {
            return Value::Null;
        }

        let bytes = field.as_bytes();

        if bytes.len() > 1 && bytes[0] == b'0' && !field.starts_with("0.") {
            return Value::String(field.to_string());
        }

        if (bytes[0] == b'+' || bytes[0] == b'-') && bytes.len() > 2 && bytes[1] == b'0' && bytes[2] != b'.' {
            return Value::String(field.to_string());
        }

        if let Ok(i) = field.parse::<i64>() {
            return Value::Number(i.into());
        }

        if let Ok(f) = field.parse::<f64>()
            && let Some(n) = Number::from_f64(f)
        {
            return Value::Number(n);
        }

        if field.eq_ignore_ascii_case("true") || field.eq_ignore_ascii_case("yes") || field.eq_ignore_ascii_case("on") {
            Value::Bool(true)
        } else if field.eq_ignore_ascii_case("false") || field.eq_ignore_ascii_case("no") || field.eq_ignore_ascii_case("off") {
            Value::Bool(false)
        } else {
            Value::String(field.to_string())
        }
    }

    /// Builds a record's data map from the header, keying each value by its column's key.
    fn keyed_by_header(column_keys: &[String], record: &StringRecord) -> Map<String, Value> {
        let mut data = Map::with_capacity(record.len());
        for (key, value) in column_keys.iter().zip(record.iter()) {
            // Every record owns its keys, since the map is handed downstream with the record.
            data.insert(key.clone(), Self::infer_value(value));
        }
        data
    }

    /// Builds a record's data map for a headerless source, keying each value by its zero-based
    /// column position, so a mapping reads column `n` as `this[n]`.
    fn keyed_by_position(record: &StringRecord) -> Map<String, Value> {
        let mut data = Map::with_capacity(record.len());
        for (index, value) in record.iter().enumerate() {
            data.insert(index.to_string(), Self::infer_value(value));
        }
        data
    }
}

impl Ingestor for CsvIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let mut reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let batch_size = self.batch_size;
        let mut batch_buffer: Vec<Record> = Vec::new();

        for result in reader.records() {
            let record = result.map_err(CsvIngestError::Reader)?;
            let data = match &self.column_keys {
                Some(column_keys) => Self::keyed_by_header(column_keys, &record),
                None => Self::keyed_by_position(&record),
            };

            batch_buffer.push(Record::new(None, data));

            if batch_buffer.len() >= batch_size {
                let batch = mem::take(&mut batch_buffer);
                sender.send(Signal::Data(batch)).map_err(|_| {
                    warn!("CSV ingestor stopping early: downstream channel closed");
                    IngestorError::ChannelClosed
                })?;
            }
        }

        if !batch_buffer.is_empty() {
            sender.send(Signal::Data(batch_buffer)).map_err(|_| {
                warn!("CSV ingestor stopping early: downstream channel closed");
                IngestorError::ChannelClosed
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{csv::ingestor::CsvIngestor, ingestor::Ingestor};
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::{
        encoding_hint::EncodingHint,
        inspectors::csv::{
            CsvInspector,
            dialect::{Dialect, LineTerminator},
            header_rows::HeaderRows,
            preamble::Preamble,
        },
        metadata::{CsvMetadata, FormatMetadata},
        profile::Profile,
    };
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use std::{fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    fn profiled_csv(contents: &str, metadata: Option<FormatMetadata>) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.csv");
        let mut file = File::create(&path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        let mut profile = Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 1.0);
        if let Some(metadata) = metadata {
            profile = profile.with_metadata(metadata);
        }
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Csv), EncodingHint::NONE)),
            profile,
        );
        (dir, payload)
    }

    fn ingest_all(payload: ProfiledPayload) -> Vec<Record> {
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let (tx, rx) = sync_channel(4);
        thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        rx.iter()
            .flat_map(|signal| {
                let Signal::Data(records) = signal else {
                    panic!("expected a data signal");
                };
                records
            })
            .collect()
    }

    #[test]
    fn each_data_row_becomes_a_record() {
        let (_dir, payload) = profiled_csv("name,age\nAda,36\nGrace,45\n", None);
        let records = ingest_all(payload);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn numeric_and_leading_zero_fields_are_inferred() {
        let (_dir, payload) = profiled_csv("name,age,zip\nAda,36,00123\nGrace,45,04510\n", None);
        let records = ingest_all(payload);
        let first = records[0].data();
        assert_eq!(first.get("age"), Some(&Value::Number(36.into())));
        // Leading-zero identifiers stay strings, not numbers.
        assert_eq!(first.get("zip"), Some(&Value::String("00123".to_string())));
    }

    #[test]
    fn every_row_ingests_without_a_collection() {
        let (_dir, payload) = profiled_csv("name,age\nAda,36\nGrace,45\n", None);
        let records = ingest_all(payload);
        assert!(records.iter().all(|record| record.collection().is_none()));
    }

    #[test]
    fn boolean_words_are_inferred_as_bools() {
        let (_dir, payload) = profiled_csv("name,active\nAda,yes\nGrace,no\n", None);
        let records = ingest_all(payload);
        assert_eq!(records[0].data().get("active"), Some(&Value::Bool(true)));
        assert_eq!(records[1].data().get("active"), Some(&Value::Bool(false)));
    }

    #[test]
    fn a_headerless_source_keys_columns_by_position() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: b',',
            quote_char: Some(b'"'),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::NONE,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: "UTF-8".to_string(),
        }));
        let (_dir, payload) = profiled_csv("1,\"Goroka\",\\N\n2,\"Madang\",\"POM\"\n", Some(metadata));
        let records = ingest_all(payload);

        // Both rows survive; nothing is consumed as a header.
        assert_eq!(records.len(), 2);
        let first = records[0].data();
        assert_eq!(first.get("0"), Some(&Value::Number(1.into())));
        assert_eq!(first.get("1"), Some(&Value::String("Goroka".to_string())));
        // `\N` is a null token, not the literal string.
        assert_eq!(first.get("2"), Some(&Value::Null));
    }

    #[test]
    fn a_backslash_n_field_is_read_as_null() {
        let (_dir, payload) = profiled_csv("name,code\nAda,\\N\n", None);
        let records = ingest_all(payload);

        assert_eq!(records[0].data().get("code"), Some(&Value::Null));
    }

    #[test]
    fn a_profiled_preamble_is_skipped_before_the_header() {
        // A quoted title spanning two lines is one CSV record: the ingestor skips it as a whole and
        // the first data row arrives intact.
        let contents = "\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2\nSTEF;12,45\nAKA;9,80\n";
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: b';',
            quote_char: Some(b'"'),
            preamble: Preamble::new(1),
            header_rows: HeaderRows::ONE,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: "UTF-8".to_string(),
        }));
        let (_dir, payload) = profiled_csv(contents, Some(metadata));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("NO2"), Some(&Value::String("12,45".to_string())));
        assert_eq!(records[1].data().get("Station"), Some(&Value::String("AKA".to_string())));
    }

    #[test]
    fn the_inspected_dialect_ingests_a_titled_table_from_its_header() {
        let contents = "Lumes;v2.10;25.10.26-08:00:00\nStation;Zeit-NO2;NO2;O3\nSTEF;25.10.2026, 08:00;12;54\nTAB;25.10.2026, 08:00;NE;38\n";
        let dialect = CsvInspector::new().inspect(contents.as_bytes(), &EncodingHint::NONE).unwrap();
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("NO2"), Some(&Value::Number(12.into())));
        assert_eq!(records[1].data().get("NO2"), Some(&Value::String("NE".to_string())));
    }

    #[test]
    fn a_stacked_header_keys_columns_by_joined_names_and_a_blank_one_by_position() {
        let contents = "Lumes;v2.10;30.10.26-10:00:00\n;Zeit-NO2;NO2;Zeit-PM10;PM10;PM10\n;;HMW;;MW24;HMW\n;MESZ;µg/m³;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;30.10.2026, 10:00;NE;NE\nTAB;30.10.2026, 10:00;NE;30.10.2026, 10:00;14,03;14,44\nKE;30.10.2026, 10:00;15,17;30.10.2026, 10:00;12,49;18,80\n";
        let dialect = CsvInspector::new().inspect(contents.as_bytes(), &EncodingHint::NONE).unwrap();
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);
        let tab = records[1].data();

        // Only data rows become records: the aggregation and unit rows are part of the header.
        assert_eq!(records.len(), 3);
        assert_eq!(tab.get("0"), Some(&Value::String("TAB".to_string())));
        assert_eq!(tab.get("Zeit-NO2 MESZ"), Some(&Value::String("30.10.2026, 10:00".to_string())));
        assert_eq!(tab.get("PM10 MW24 µg/m³"), Some(&Value::String("14,03".to_string())));
        assert_eq!(tab.get("PM10 HMW µg/m³"), Some(&Value::String("14,44".to_string())));
        assert_eq!(tab.len(), 6);
    }

    #[test]
    fn profiled_dialect_is_used_for_non_default_csv() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: b';',
            quote_char: Some(b'"'),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: "UTF-8".to_string(),
        }));
        let (_dir, payload) = profiled_csv("name;age\nAda;36\nGrace;45\n", Some(metadata));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("age"), Some(&Value::Number(36.into())));
    }
}

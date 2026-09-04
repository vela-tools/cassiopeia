use crate::{csv::error::CsvIngestError, error::IngestorError, ingestor::Ingestor};
use ::csv::{Reader, ReaderBuilder, StringRecord};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_data_profiler::metadata::FormatMetadata;
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use encoding_rs::{Encoding, UTF_8};
use encoding_rs_io::{DecodeReaderBytes, DecodeReaderBytesBuilder};
use rustc_hash::FxHashMap;
use serde_json::{Map, Number, Value};
use std::{fs::File, mem};
use tracing::{info, warn};

/// Ingestor for CSV data.
///
/// Uses the profiled CSV dialect when one is available, then parses records with
/// smart value inference.
pub struct CsvIngestor {
    reader: Option<Reader<DecodeReaderBytes<File, Vec<u8>>>>,
    header_mapping: FxHashMap<String, String>,
    has_headers: bool,
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

        let mut builder = ReaderBuilder::new();
        let encoding = if let Some(dialect) = &dialect {
            info!("Using profiled CSV dialect: {:#?}", dialect);
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

            Encoding::for_label(dialect.encoding.as_bytes()).unwrap_or(UTF_8)
        } else {
            UTF_8
        };

        // Decode the file to UTF-8 with the profiled encoding, so a non-UTF-8 source (Windows-1252,
        // Latin-1, ...) is read correctly instead of failing on its first non-UTF-8 byte.
        let file = File::open(&path).map_err(|source| CsvIngestError::Open {
            source,
            // The error owns the path after this borrow ends.
            path: path.clone(),
        })?;
        let decoded = DecodeReaderBytesBuilder::new().encoding(Some(encoding)).build(file);
        let mut reader = builder.from_reader(decoded);

        // A headerless source has no names to key its columns by; its records are keyed by position
        // instead, so the first row is not consumed as a header and nothing is lost.
        let has_headers = dialect.as_ref().is_none_or(|dialect| dialect.has_headers);
        let header_mapping = if has_headers {
            let headers = reader.headers().map_err(CsvIngestError::Reader)?.iter().map(str::to_string).collect::<Vec<_>>();
            let mapping = Self::create_header_mapping(&headers);
            info!("Header mapping: {:#?}", mapping);
            mapping
        } else {
            info!("Headerless CSV: columns keyed by position");
            FxHashMap::default()
        };

        Ok(CsvIngestor {
            reader: Some(reader),
            header_mapping,
            has_headers,
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

    fn create_header_mapping(headers: &[String]) -> FxHashMap<String, String> {
        headers.iter().map(|original| (original.clone(), Self::clean_header(original))).collect()
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

    /// Builds a record's data map from a header row, keying each value by its header name.
    fn keyed_by_header(headers: &StringRecord, header_mapping: &FxHashMap<String, String>, record: &StringRecord) -> Map<String, Value> {
        let mut data = Map::with_capacity(record.len());
        for (header, value) in headers.iter().zip(record.iter()) {
            let cleaned = header_mapping.get(header).map_or(header, String::as_str);
            data.insert(cleaned.to_string(), Self::infer_value(value));
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
        let header_mapping = self.header_mapping;
        let batch_size = self.batch_size;
        let mut batch_buffer: Vec<Record> = Vec::new();

        // A headerless source keys its columns by position; otherwise the header row names them. The
        // header row is read once here so it is not treated as data.
        let headers = if self.has_headers {
            Some(reader.headers().map_err(CsvIngestError::Reader)?.clone())
        } else {
            None
        };

        for result in reader.records() {
            let record = result.map_err(CsvIngestError::Reader)?;
            let data = match &headers {
                Some(headers) => Self::keyed_by_header(headers, &header_mapping, &record),
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
        inspectors::csv::dialect::{Dialect, LineTerminator},
        metadata::{CsvMetadata, FormatMetadata},
        profile::Profile,
    };
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
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
        let payload = ProfiledPayload::new(CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Csv))), profile);
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
            has_headers: false,
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
    fn profiled_dialect_is_used_for_non_default_csv() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: b';',
            quote_char: Some(b'"'),
            has_headers: true,
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

use crate::{
    csv::{
        column_keys::ColumnKeys,
        error::{CsvIngestError, CsvRead},
        line_counting_reader::LineCountingReader,
        misshapen_rows::MisshapenRows,
    },
    error::IngestorError,
    ingestor::Ingestor,
    record_batches::RecordBatches,
};
use ::csv::StringRecord;
use cassiopeia_common::{channel::ChannelSender, input::Input, signal::Signal};
use cassiopeia_csv_column::{column_key::ColumnKey, renamed_column::RenamedColumn};
use cassiopeia_csv_dialect::{
    decimal_separator::DecimalSeparator,
    dialect::Dialect,
    line_terminator::LineTerminator,
    table_reader::{TableReader, TableRowError},
    value_notation::ValueNotation,
};
use cassiopeia_data_profiler::metadata::{CsvMetadata, FormatMetadata};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use encoding_rs::{Encoding, UTF_8};
use encoding_rs_rw::DecodingReader;
use parking_lot::Mutex;
use serde_json::{Map, Number, Value};
use std::{
    borrow::Cow,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, warn};

/// A file decoded to UTF-8 from its profiled encoding, its lines counted.
type DecodedFile = BufReader<LineCountingReader<DecodingReader<BufReader<File>>>>;

/// A CSV table reader over a [`DecodedFile`].
type DecodedFileReader = TableReader<DecodedFile>;

/// Ingestor for CSV data.
///
/// Uses the profiled CSV dialect when one is available, then parses records with
/// smart value inference.
pub struct CsvIngestor {
    reader: Option<DecodedFileReader>,
    /// The file the records are read from.
    path: PathBuf,
    /// The input the file was collected from, which diagnostics name it by.
    source: Input,
    /// The record key of each column, from the header; `None` for a headerless source, whose
    /// columns are keyed by position.
    column_keys: Option<ColumnKeys>,
    /// The encoding the file is decoded from, named when a line of the file is not valid in it.
    encoding: &'static Encoding,
    /// How the file writes its values: its decimal separator and null tokens.
    notation: ValueNotation,
    /// The rows skipped so far because their field count differs from the table's, shared with
    /// whoever reports them once the file has been read.
    misshapen: Arc<Mutex<MisshapenRows>>,
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
        let (payload, profile) = payload.into_parts();
        let dialect = profile.into_metadata().and_then(FormatMetadata::into_csv).map(CsvMetadata::into_dialect);

        let (path, source) = match payload {
            CollectedPayload::File(file) => file.into_path_and_source(),
            CollectedPayload::Bytes(_) => return Err(CsvIngestError::RequiresFile.into()),
        };

        let encoding = dialect.as_ref().map_or(UTF_8, Dialect::character_encoding);
        let terminator = dialect.as_ref().map_or(LineTerminator::Lf, |dialect| dialect.terminator);
        let decoded = Self::open_decoded(&path, encoding, terminator)?;
        let mut reader = Self::table_reader(decoded, dialect.as_ref(), encoding, &path)?;
        let column_keys = Self::column_keys(&mut reader, dialect.as_ref(), encoding, &path)?;

        // An unprofiled file states no notation, so its values are read in the plain one.
        let notation = dialect.map_or(ValueNotation::PLAIN, |dialect| dialect.notation);

        Ok(CsvIngestor {
            reader: Some(reader),
            path,
            source,
            column_keys,
            encoding,
            notation,
            misshapen: Arc::default(),
            batch_size,
        })
    }

    /// Opens the file at `path` decoded from `encoding` to UTF-8, counting its lines by `terminator`.
    ///
    /// The file is decoded strictly: a byte sequence that is not valid in its encoding fails the run
    /// on its line, rather than reaching the mapped values as `U+FFFD`. A byte order mark names the
    /// encoding over the profiled one, as for any decoder of the web's encodings.
    fn open_decoded(path: &Path, encoding: &'static Encoding, terminator: LineTerminator) -> Result<DecodedFile, CsvIngestError> {
        let file = File::open(path).map_err(|source| CsvIngestError::Open {
            source,
            // The error outlives the borrowed path.
            path: path.to_path_buf(),
        })?;
        Ok(BufReader::new(LineCountingReader::new(
            DecodingReader::new(BufReader::new(file), encoding.new_decoder()),
            terminator,
        )))
    }

    /// The reader of the table in `decoded`, positioned at its header.
    ///
    /// The profiled dialect opens the file exactly as the inspector parsed its sample, preamble skip
    /// and stacked-header join included, so the header read next is the one the inspector detected.
    /// An unprofiled file is read with the `csv` crate's defaults.
    fn table_reader(decoded: DecodedFile, dialect: Option<&Dialect>, encoding: &'static Encoding, path: &Path) -> Result<DecodedFileReader, CsvIngestError> {
        match dialect {
            Some(dialect) => {
                info!("Using profiled CSV dialect: {:#?}", dialect);
                dialect
                    .reader(decoded)
                    .map_err(|source| CsvIngestError::reading(source, encoding, path, CsvRead::TableStart))
            }
            None => TableReader::with_csv_defaults(decoded).map_err(|source| CsvIngestError::reading(source, encoding, path, CsvRead::TableStart)),
        }
    }

    /// The record key of every column, from the header `reader` is positioned at, or `None` for a
    /// headerless source.
    ///
    /// A headerless source has no names to key its columns by; its records are keyed by position
    /// instead, so the first row is not consumed as a header and nothing is lost.
    fn column_keys(
        reader: &mut DecodedFileReader,
        dialect: Option<&Dialect>,
        encoding: &'static Encoding,
        path: &Path,
    ) -> Result<Option<ColumnKeys>, CsvIngestError> {
        if dialect.is_some_and(|dialect| !dialect.header_rows.is_present()) {
            info!("Headerless CSV: columns keyed by position");
            return Ok(None);
        }
        let headers = reader
            .headers()
            .map_err(|source| CsvIngestError::reading(source, encoding, path, CsvRead::Table))?;
        let keys = ColumnKeys::from_headers(headers);
        info!("Column keys: {:#?}", keys);
        Ok(Some(keys))
    }

    /// The input the file was collected from: the file itself, or the URL a downloaded file was
    /// fetched from, which stays the same from one run to the next where the downloaded copy's path
    /// does not.
    #[must_use]
    pub const fn source(&self) -> &Input {
        &self.source
    }

    /// The columns whose header asks for a key another column holds, each read by a generated key
    /// instead so its values are not lost. Empty for a header without repeats and for a headerless
    /// source.
    #[must_use]
    pub fn renamed_columns(&self) -> &[RenamedColumn] {
        self.column_keys.as_ref().map_or(&[], ColumnKeys::renamed)
    }

    /// The rows the ingest skips because their field count differs from the table's, filled in as
    /// the file is read: complete once [`Ingestor::ingest`] has returned.
    #[must_use]
    pub fn misshapen_rows(&self) -> Arc<Mutex<MisshapenRows>> {
        Arc::clone(&self.misshapen)
    }

    /// Smart value inference for a CSV field, under the file's value notation.
    ///
    /// A blank field and a null token are a missing value. Leading-zero numbers are kept as strings
    /// (identifiers such as postal codes), otherwise integers, floats written with the notation's
    /// decimal separator, and booleans are parsed out.
    fn infer_value(field: &str, notation: &ValueNotation) -> Value {
        let field = field.trim();
        if field.is_empty() {
            return Value::Null;
        }

        // `\N` is the null token emitted by SQL/`COPY`-style CSV dumps (OpenFlights among them); it
        // is an absent value, not the literal two-character string. A file states its own tokens.
        if field == r"\N" || notation.is_null_token(field) {
            return Value::Null;
        }

        let bytes = field.as_bytes();
        let separator = notation.decimal_separator.byte();

        if bytes.len() > 1 && bytes[0] == b'0' && bytes[1] != separator {
            return Value::String(field.to_string());
        }

        if (bytes[0] == b'+' || bytes[0] == b'-') && bytes.len() > 2 && bytes[1] == b'0' && bytes[2] != separator {
            return Value::String(field.to_string());
        }

        if let Some(number) = Self::number(field, notation.decimal_separator) {
            return Value::Number(number);
        }

        if field.eq_ignore_ascii_case("true") || field.eq_ignore_ascii_case("yes") || field.eq_ignore_ascii_case("on") {
            Value::Bool(true)
        } else if field.eq_ignore_ascii_case("false") || field.eq_ignore_ascii_case("no") || field.eq_ignore_ascii_case("off") {
            Value::Bool(false)
        } else {
            Value::String(field.to_string())
        }
    }

    /// Parses `field` as an integer, or as a float written with `separator`.
    ///
    /// Under a comma separator a full stop belongs to no number, so a field holding one, such as the
    /// date `25.10.2026` or the grouped `1.234`, stays text rather than misreading as a fraction.
    fn number(field: &str, separator: DecimalSeparator) -> Option<Number> {
        let text = match separator {
            DecimalSeparator::Comma if field.contains('.') => return None,
            DecimalSeparator::Comma if field.contains(',') => Cow::Owned(field.replacen(',', ".", 1)),
            DecimalSeparator::Dot | DecimalSeparator::Comma => Cow::Borrowed(field),
        };

        if let Ok(integer) = text.parse::<i64>() {
            return Some(integer.into());
        }
        text.parse::<f64>().ok().and_then(Number::from_f64)
    }

    /// Builds a record's data map from the header, keying each value by its column's key.
    fn keyed_by_header(column_keys: &[ColumnKey], record: &StringRecord, notation: &ValueNotation) -> Map<String, Value> {
        let mut data = Map::with_capacity(record.len());
        for (key, value) in column_keys.iter().zip(record.iter()) {
            let key: &str = key.as_ref();
            // Every record owns its keys, since the map is handed downstream with the record.
            data.insert(key.to_owned(), Self::infer_value(value, notation));
        }
        data
    }

    /// Builds a record's data map for a headerless source, keying each value by its zero-based
    /// column position, so a mapping reads column `n` as `this[n]`.
    fn keyed_by_position(record: &StringRecord, notation: &ValueNotation) -> Map<String, Value> {
        let mut data = Map::with_capacity(record.len());
        for (index, value) in record.iter().enumerate() {
            data.insert(index.to_string(), Self::infer_value(value, notation));
        }
        data
    }
}

impl Ingestor for CsvIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let mut reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let mut batches = RecordBatches::new(&sender, self.batch_size);
        let channel_closed = |_: &IngestorError| warn!("CSV ingestor stopping early: downstream channel closed");

        let encoding = self.encoding;
        for result in reader.records() {
            let record = match result {
                Ok(record) => record,
                Err(TableRowError::Misshapen(row)) => {
                    self.misshapen.lock().record(row);
                    continue;
                }
                Err(TableRowError::Csv(source)) => return Err(CsvIngestError::reading(source, encoding, &self.path, CsvRead::Table).into()),
            };
            let data = match &self.column_keys {
                Some(column_keys) => Self::keyed_by_header(column_keys.keys(), &record, &self.notation),
                None => Self::keyed_by_position(&record, &self.notation),
            };

            batches.push(Record::new(None, data)).inspect_err(channel_closed)?;
        }
        batches.finish().inspect_err(channel_closed)
    }
}

#[cfg(test)]
mod tests {
    use crate::{csv::ingestor::CsvIngestor, ingestor::Ingestor};
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_csv_column::{column_key::ColumnKey, column_number::ColumnNumber, renamed_column::RenamedColumn};
    use cassiopeia_csv_dialect::{
        character_encoding::CharacterEncoding,
        decimal_separator::DecimalSeparator,
        dialect::Dialect,
        dialect_byte::DialectByte,
        escape::Escape,
        header_rows::HeaderRows,
        line_terminator::LineTerminator,
        misshapen_row::MisshapenRow,
        null_token::NullToken,
        preamble::Preamble,
        quote::Quote,
        row_number::RowNumber,
        statement::DialectStatement,
        value_notation::ValueNotation,
    };
    use cassiopeia_data_profiler::{
        content::Content,
        inspectors::csv::CsvInspector,
        metadata::{CsvMetadata, FormatMetadata},
        profile::Profile,
    };
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use encoding_rs::{Encoding, UTF_8, WINDOWS_1252, WINDOWS_1253};
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use std::{fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    fn profiled_csv(contents: impl AsRef<[u8]>, metadata: Option<FormatMetadata>) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.csv");
        let mut file = File::create(&path).unwrap();
        file.write_all(contents.as_ref()).unwrap();
        let mut profile = Profile::new(DataFormat::Csv, media_type!(TEXT / CSV), 1.0);
        if let Some(metadata) = metadata {
            profile = profile.with_metadata(metadata);
        }
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(path, Some(DataFormat::Csv), EncodingHint::NONE)),
            profile,
        );
        (dir, payload)
    }

    fn ingest_all(payload: ProfiledPayload) -> Vec<Record> {
        ingest(CsvIngestor::from_payload(payload, 8).unwrap())
    }

    fn ingest(ingestor: CsvIngestor) -> Vec<Record> {
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
            delimiter: DialectByte::COMMA,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::NONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
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
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::new(1),
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
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
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(contents.as_bytes()))
            .unwrap();
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("NO2"), Some(&Value::Number(12.into())));
        assert_eq!(records[1].data().get("NO2"), Some(&Value::String("NE".to_string())));
    }

    #[test]
    fn the_inspected_dialect_ingests_the_one_data_row_of_a_titled_all_text_table() {
        let (_dir, payload) = inspected("Lumes;v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\n");
        let records = ingest_all(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("NO2"), Some(&Value::String("12,45".to_string())));
        assert_eq!(records[0].data().get("O3"), Some(&Value::String("54,55".to_string())));
    }

    #[test]
    fn a_file_whose_lines_end_with_a_lone_carriage_return_ingests_a_record_per_line() {
        let (_dir, payload) = inspected(b"id,name\r1,Ada\r2,Grace\r");
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("name"), Some(&Value::String("Ada".to_string())));
        assert_eq!(records[1].data().get("id"), Some(&Value::Number(2.into())));
    }

    #[test]
    fn a_single_column_file_ingests_a_record_per_row_keyed_by_its_header() {
        let (_dir, payload) = inspected(b"name\nAda\nGrace\nLinus\n");
        let records = ingest_all(payload);
        let names: Vec<&Value> = records.iter().filter_map(|record| record.data().get("name")).collect();

        assert_eq!(names, [&Value::from("Ada"), &Value::from("Grace"), &Value::from("Linus")]);
    }

    #[test]
    fn a_utf_16le_file_without_a_byte_order_mark_ingests_its_text_intact() {
        let bytes: Vec<u8> = "id,name\n1,Čapek\n2,Grace\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let (_dir, payload) = inspected(bytes);
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("name"), Some(&Value::String("Čapek".to_string())));
        assert_eq!(records[1].data().get("id"), Some(&Value::Number(2.into())));
    }

    #[test]
    fn a_quoted_two_line_title_in_a_utf_16le_file_with_a_byte_order_mark_ingests_every_data_row_intact() {
        let text = "\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\nAKA;9,80;40,10\n";
        let bytes: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let (_dir, payload) = inspected(bytes);
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("O3"), Some(&Value::String("54,55".to_string())));
        assert_eq!(records[1].data().get("Station"), Some(&Value::String("AKA".to_string())));
    }

    #[test]
    fn a_pipe_table_padded_for_reading_is_keyed_and_valued_without_its_padding() {
        let contents = "id | name | count\n1 | Ada | 36\n2 | Grace | 45\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(contents.as_bytes()))
            .unwrap();
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("id"), Some(&Value::Number(1.into())));
        assert_eq!(records[0].data().get("name"), Some(&Value::String("Ada".to_string())));
        assert_eq!(records[1].data().get("count"), Some(&Value::Number(45.into())));
    }

    #[test]
    fn the_inspected_preamble_of_a_quoted_two_line_title_ingests_the_first_data_row_intact() {
        // Five line feeds but four CSV records: the inspector and the ingestor must both count the
        // quoted title as one record, or the header or the first data row is lost.
        let contents = "\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\nAKA;9,80;40,10\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(contents.as_bytes()))
            .unwrap();
        assert_eq!(dialect.preamble, Preamble::new(1));
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("NO2"), Some(&Value::String("12,45".to_string())));
        assert_eq!(records[0].data().get("O3"), Some(&Value::String("54,55".to_string())));
        assert_eq!(records[1].data().get("Station"), Some(&Value::String("AKA".to_string())));
    }

    #[test]
    fn the_inspected_backslash_escape_ingests_escaped_quotes_as_quotes() {
        let contents = "id,name,value\n1,\"say \\\"hi\\\"\",10\n2,plain,20\n3,\"a\\\"b\",30\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(contents.as_bytes()))
            .unwrap();
        assert_eq!(dialect.escape, Escape::Char(DialectByte::BACKSLASH));
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);
        let names: Vec<_> = records.iter().map(|record| record.data().get("name").and_then(Value::as_str)).collect();

        assert_eq!(names, vec![Some("say \"hi\""), Some("plain"), Some("a\"b")]);
    }

    #[test]
    fn a_stacked_header_keys_columns_by_joined_names_and_a_blank_one_by_position() {
        let contents = "Lumes;v2.10;30.10.26-10:00:00\n;Zeit-NO2;NO2;Zeit-PM10;PM10;PM10\n;;HMW;;MW24;HMW\n;MESZ;µg/m³;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;30.10.2026, 10:00;NE;NE\nTAB;30.10.2026, 10:00;NE;30.10.2026, 10:00;14,03;14,44\nKE;30.10.2026, 10:00;15,17;30.10.2026, 10:00;12,49;18,80\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(contents.as_bytes()))
            .unwrap();
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

    fn dialect_in(encoding: &'static Encoding) -> FormatMetadata {
        FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(encoding),
            notation: ValueNotation::PLAIN,
        }))
    }

    fn inspected(contents: impl AsRef<[u8]>) -> (TempDir, ProfiledPayload) {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(contents.as_ref())).unwrap();
        profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))))
    }

    fn skipped_row(above: u64, fields: usize, table_fields: usize) -> MisshapenRow {
        MisshapenRow {
            row: RowNumber::after(above),
            fields,
            table_fields,
        }
    }

    #[test]
    fn a_short_and_a_long_row_are_skipped_and_the_rows_around_them_ingested() {
        let (_dir, payload) = inspected("id;name;value\n1;a;10\n2;b\n3;c;30\n4;d;40;x\n5;e;50\n");
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let misshapen = ingestor.misshapen_rows();
        let records = ingest(ingestor);

        let ids: Vec<_> = records.iter().filter_map(|record| record.data().get("id")).collect();
        assert_eq!(ids, [&Value::Number(1.into()), &Value::Number(3.into()), &Value::Number(5.into())]);
        let skipped = misshapen.lock();
        assert_eq!(skipped.named(), [skipped_row(2, 2, 3), skipped_row(4, 4, 3)]);
        assert_eq!(skipped.total(), 2);
    }

    #[test]
    fn a_footer_note_below_a_titled_table_is_skipped_on_its_row_of_the_file() {
        let (_dir, payload) = inspected("Air quality\nid;name;value\n1;a;10\n2;b;20\n3;c;30\nSource: City statistics office\n");
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let misshapen = ingestor.misshapen_rows();
        let records = ingest(ingestor);

        assert_eq!(records.len(), 3);
        assert_eq!(misshapen.lock().named(), [skipped_row(5, 1, 3)]);
    }

    #[test]
    fn an_unprofiled_file_skips_a_misshapen_row_too() {
        let (_dir, payload) = profiled_csv("id,name\n1,a\n2\n3,c\n", None);
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let misshapen = ingestor.misshapen_rows();
        let records = ingest(ingestor);

        assert_eq!(records.len(), 2);
        assert_eq!(misshapen.lock().named(), [skipped_row(2, 1, 2)]);
    }

    #[test]
    fn a_header_ending_in_a_delimiter_ingests_every_row_without_one() {
        let (_dir, payload) = inspected("id;name;value;\n1;a;10\n2;b;20\n3;c;30\n");
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let misshapen = ingestor.misshapen_rows();
        let records = ingest(ingestor);

        let values: Vec<_> = records.iter().filter_map(|record| record.data().get("value")).collect();
        assert_eq!(values, [&Value::Number(10.into()), &Value::Number(20.into()), &Value::Number(30.into())]);
        assert_eq!(misshapen.lock().total(), 0);
    }

    #[test]
    fn rows_ending_in_a_delimiter_ingest_under_a_header_without_one_with_no_empty_column() {
        let (_dir, payload) = inspected("id;name;value\n1;a;10;\n2;b;20;\n3;c;30;\n");
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let misshapen = ingestor.misshapen_rows();
        let records = ingest(ingestor);

        assert_eq!(records.len(), 3);
        assert_eq!(data_keys(&records[0]), ["id", "name", "value"]);
        assert_eq!(misshapen.lock().total(), 0);
    }

    #[test]
    fn a_file_without_misshapen_rows_skips_nothing() {
        let (_dir, payload) = inspected("id;name\n1;a\n2;b\n3;c\n");
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let misshapen = ingestor.misshapen_rows();
        let records = ingest(ingestor);

        assert_eq!(records.len(), 3);
        assert_eq!(misshapen.lock().total(), 0);
    }

    fn ingest_error(payload: ProfiledPayload) -> String {
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        let (tx, rx) = sync_channel(4);
        let ingest = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        rx.iter().for_each(drop);
        ingest.join().unwrap().unwrap_err().to_string()
    }

    #[test]
    fn a_utf_8_file_with_a_byte_that_is_not_utf_8_fails_naming_its_line() {
        let (dir, payload) = profiled_csv(b"Station;Name\nSTEF;Cafe\nAKA;M\xfcller\n", Some(dialect_in(UTF_8)));

        assert_eq!(
            ingest_error(payload),
            format!("line 3 of the CSV file at '{}' is not valid UTF-8", dir.path().join("data.csv").display())
        );
    }

    #[test]
    fn a_byte_with_no_character_in_a_single_byte_encoding_fails_naming_its_line() {
        // 0xD2 has no character in windows-1253.
        let (dir, payload) = profiled_csv(b"Station;Name\nSTEF;\xd2\n", Some(dialect_in(WINDOWS_1253)));

        assert_eq!(
            ingest_error(payload),
            format!(
                "line 2 of the CSV file at '{}' is not valid windows-1253",
                dir.path().join("data.csv").display()
            )
        );
    }

    #[test]
    fn a_header_below_a_title_with_a_byte_that_is_not_utf_8_fails_naming_its_file_line() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::new(1),
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }));
        let (dir, payload) = profiled_csv(b"\"Lumes\nreport\";v2\nStation;\xb5g\nSTEF;12\n", Some(metadata));
        let Err(error) = CsvIngestor::from_payload(payload, 8) else {
            panic!("expected the header to fail");
        };

        assert_eq!(
            error.to_string(),
            format!("line 3 of the CSV file at '{}' is not valid UTF-8", dir.path().join("data.csv").display())
        );
    }

    #[test]
    fn a_stated_stacked_header_with_a_unit_row_one_field_short_ingests_every_data_row() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::new(3),
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }));
        let (_dir, payload) = profiled_csv("Station;NO2;O3\n;HMW;HMW\n;µg/m³\nSTEF;18;50\nTAB;9;10\n", Some(metadata));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data()["Station"], Value::from("STEF"));
        assert_eq!(records[0].data()["NO2 HMW µg/m³"], Value::from(18));
        assert_eq!(records[1].data()["O3 HMW"], Value::from(10));
    }

    #[test]
    fn a_windows_1252_file_is_transcoded_to_utf_8() {
        let (_dir, payload) = profiled_csv(b"Station;Unit\nSTEF;\xb5g/m\xb3\n", Some(dialect_in(WINDOWS_1252)));
        let records = ingest_all(payload);

        assert_eq!(records[0].data().get("Unit"), Some(&Value::String("µg/m³".to_string())));
    }

    #[test]
    fn a_utf_8_byte_order_mark_is_not_part_of_the_first_column_key() {
        let (_dir, payload) = profiled_csv(b"\xef\xbb\xbfStation;Name\nSTEF;Caf\xc3\xa9\n", Some(dialect_in(UTF_8)));
        let records = ingest_all(payload);

        assert_eq!(records[0].data().get("Station"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("Name"), Some(&Value::String("Café".to_string())));
    }

    #[test]
    fn profiled_dialect_is_used_for_non_default_csv() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }));
        let (_dir, payload) = profiled_csv("name;age\nAda;36\nGrace;45\n", Some(metadata));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("age"), Some(&Value::Number(36.into())));
    }

    fn semicolon_dialect_with(notation: ValueNotation) -> FormatMetadata {
        FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation,
        }))
    }

    #[test]
    fn a_stated_comma_decimal_reads_as_a_number_and_a_dotted_date_stays_text() {
        let notation = ValueNotation {
            decimal_separator: DecimalSeparator::Comma,
            ..ValueNotation::PLAIN
        };
        let (_dir, payload) = profiled_csv(
            "Station;NO2;Zip;Date\nSTEF;12,45;0123;25.10.2026\nAKA;0,5;1010;25.10\n",
            Some(semicolon_dialect_with(notation)),
        );
        let records = ingest_all(payload);

        assert_eq!(records[0].data().get("NO2"), Some(&serde_json::json!(12.45)));
        assert_eq!(records[1].data().get("NO2"), Some(&serde_json::json!(0.5)));
        assert_eq!(records[0].data().get("Zip"), Some(&Value::String("0123".to_string())));
        assert_eq!(records[1].data().get("Zip"), Some(&Value::Number(1010.into())));
        assert_eq!(records[0].data().get("Date"), Some(&Value::String("25.10.2026".to_string())));
        assert_eq!(records[1].data().get("Date"), Some(&Value::String("25.10".to_string())));
    }

    #[test]
    fn a_stated_null_token_reads_as_null_and_any_other_text_stays_text() {
        let notation = ValueNotation {
            null_values: vec![NullToken::try_from("NE".to_owned()).unwrap(), NullToken::try_from("---".to_owned()).unwrap()],
            ..ValueNotation::PLAIN
        };
        let (_dir, payload) = profiled_csv("Station;NO2;O3\nTAB; NE ;---\nAKA;NEU;12\n", Some(semicolon_dialect_with(notation)));
        let records = ingest_all(payload);

        assert_eq!(records[0].data().get("NO2"), Some(&Value::Null));
        assert_eq!(records[0].data().get("O3"), Some(&Value::Null));
        assert_eq!(records[1].data().get("NO2"), Some(&Value::String("NEU".to_string())));
    }

    fn renamed(wanted: &str, holder: usize, column: usize, key: &str) -> RenamedColumn {
        RenamedColumn {
            wanted: ColumnKey::from(wanted.to_owned()),
            holder: ColumnNumber::of_index(holder),
            column: ColumnNumber::of_index(column),
            key: ColumnKey::from(key.to_owned()),
        }
    }

    fn data_keys(record: &Record) -> Vec<&str> {
        record.data().keys().map(String::as_str).collect()
    }

    #[test]
    fn a_repeated_header_keeps_both_columns_and_reports_the_renamed_one() {
        let (_dir, payload) = profiled_csv("id;value;value\n1;10;11\n2;20;21\n3;30;31\n", Some(dialect_in(UTF_8)));
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        assert_eq!(ingestor.renamed_columns(), [renamed("value", 1, 2, "value@3")]);

        let records = ingest(ingestor);
        assert_eq!(records.len(), 3);
        assert_eq!(data_keys(&records[0]), ["id", "value", "value@3"]);
        let values: Vec<(&Value, &Value)> = records.iter().map(|record| (&record.data()["value"], &record.data()["value@3"])).collect();
        assert_eq!(
            values,
            [
                (&Value::Number(10.into()), &Value::Number(11.into())),
                (&Value::Number(20.into()), &Value::Number(21.into())),
                (&Value::Number(30.into()), &Value::Number(31.into())),
            ]
        );
    }

    #[test]
    fn a_blank_header_keyed_by_the_position_a_named_column_holds_is_renamed() {
        let (_dir, payload) = profiled_csv("2;name;\nSTEF;Stefansplatz;12\n", Some(dialect_in(UTF_8)));
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        assert_eq!(ingestor.renamed_columns(), [renamed("2", 0, 2, "2@3")]);

        let records = ingest(ingestor);
        assert_eq!(data_keys(&records[0]), ["2", "name", "2@3"]);
        assert_eq!(records[0].data().get("2"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("2@3"), Some(&Value::Number(12.into())));
    }

    #[test]
    fn a_stacked_header_joining_two_columns_to_one_name_keeps_both_columns() {
        let contents = ";NO2;NO2\n;HMW;HMW\n;µg/m³;µg/m³\nSTEF;12;13\n";
        let statement: DialectStatement = serde_json::from_value(serde_json::json!({
            "delimiter": ";",
            "skipRows": 0,
            "headerRows": 3,
        }))
        .unwrap();
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(contents.as_bytes())).unwrap();
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        assert_eq!(ingestor.renamed_columns(), [renamed("NO2 HMW µg/m³", 1, 2, "NO2 HMW µg/m³@3")]);

        let records = ingest(ingestor);
        assert_eq!(data_keys(&records[0]), ["0", "NO2 HMW µg/m³", "NO2 HMW µg/m³@3"]);
        assert_eq!(records[0].data().get("NO2 HMW µg/m³"), Some(&Value::Number(12.into())));
        assert_eq!(records[0].data().get("NO2 HMW µg/m³@3"), Some(&Value::Number(13.into())));
    }

    #[test]
    fn a_header_without_repeats_keeps_its_keys_and_renames_nothing() {
        let (_dir, payload) = profiled_csv("id;name;\n1;Ada;x\n", Some(dialect_in(UTF_8)));
        let ingestor = CsvIngestor::from_payload(payload, 8).unwrap();
        assert_eq!(ingestor.renamed_columns(), []);

        let records = ingest(ingestor);
        assert_eq!(data_keys(&records[0]), ["id", "name", "2"]);
    }

    #[test]
    fn a_headerless_source_with_repeated_values_renames_nothing() {
        let metadata = FormatMetadata::Csv(CsvMetadata::new(Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::NONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }));
        let (_dir, payload) = profiled_csv("value;value\n1;2\n", Some(metadata));

        assert_eq!(CsvIngestor::from_payload(payload, 8).unwrap().renamed_columns(), []);
    }

    #[test]
    fn a_stated_dialect_ingests_a_titled_stacked_table_with_comma_decimals_and_null_tokens() {
        let contents = "Lumes;v2.10;30.10.26-10:00:00\n;Zeit-NO2;NO2;PM10\n;;HMW;MW24\n;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;NE\nTAB;30.10.2026, 10:00;---;14,03\n";
        let statement: DialectStatement = serde_json::from_value(serde_json::json!({
            "delimiter": ";",
            "skipRows": 1,
            "headerRows": 3,
            "decimalSeparator": ",",
            "nullValues": ["NE", "---"],
        }))
        .unwrap();
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(contents.as_bytes())).unwrap();
        let (_dir, payload) = profiled_csv(contents, Some(FormatMetadata::Csv(CsvMetadata::new(dialect))));
        let records = ingest_all(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("0"), Some(&Value::String("STEF".to_string())));
        assert_eq!(records[0].data().get("NO2 HMW µg/m³"), Some(&serde_json::json!(18.48)));
        assert_eq!(records[0].data().get("PM10 MW24 µg/m³"), Some(&Value::Null));
        assert_eq!(records[1].data().get("NO2 HMW µg/m³"), Some(&Value::Null));
        assert_eq!(records[1].data().get("PM10 MW24 µg/m³"), Some(&serde_json::json!(14.03)));
    }
}

use crate::inspectors::csv::{
    data_type::DataType,
    dialect::{Dialect, LineTerminator},
    error::CsvInspectError,
};
use ::csv::{Reader, ReaderBuilder, StringRecord};
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
};

pub mod data_type;
pub mod dialect;
pub mod error;

/// Table structure for uniformity analysis during dialect scoring.
#[derive(Debug)]
struct Table {
    records: Vec<StringRecord>,
    column_types: Vec<Vec<DataType>>,
    num_columns: usize,
    num_rows: usize,
}

/// Whether a candidate parse treats row 0 as a header to skip or as an ordinary data row to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderRow {
    /// Row 0 is data and stays in the table.
    Keep,
    /// Row 0 is a header and is not read as data.
    Skip,
}

/// Read granularity for [`CsvInspector::read_sample`].
const CHUNK: usize = 64 * 1024;

/// The fewest complete rows a sample must hold before header scoring can run: row 0 plus enough
/// data rows for both the header hypothesis (which needs two full rows) and per-column typing.
const MIN_SAMPLE_ROWS: usize = 5;

/// Hard ceiling on the dialect sample. It bounds the read, and the scoring work of the candidate
/// dialects, for a file whose rows are pathologically wide: a single row can exceed the soft
/// target on its own, so a fixed-size sample could hold no complete data row at all.
const MAX_SAMPLE_BYTES: usize = 4 * 1024 * 1024;

/// CSV dialect inspector.
///
/// The structural inspector for delimited text: it samples a payload and scores candidate dialects to
/// recover the delimiter, quote, header, and terminator a specific CSV file uses. It runs whichever way
/// the format was decided, as part of auto-detection and for a declared CSV alike.
pub struct CsvInspector {
    delimiters: Vec<u8>,
    quotes: Vec<Option<u8>>,
    sample_size: usize,
}

impl Default for CsvInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvInspector {
    /// Creates an inspector with the default candidate delimiters and quotes.
    #[must_use]
    pub fn new() -> CsvInspector {
        CsvInspector {
            delimiters: vec![b',', b'\t', b';', b'|', b' '],
            quotes: vec![Some(b'"'), Some(b'\''), None],
            sample_size: 32 * 1024,
        }
    }

    /// Inspects the dialect of `reader` by sampling and scoring candidate dialects.
    ///
    /// # Errors
    ///
    /// Returns [`CsvInspectError`] when the sample cannot be read, is empty, or no candidate
    /// dialect produces a coherent table.
    pub fn inspect<R: Read>(&self, reader: R) -> Result<Dialect, CsvInspectError> {
        let mut sample_buf = self.read_sample(reader)?;

        if sample_buf.is_empty() {
            return Err(CsvInspectError::EmptyInput);
        }

        // Truncate to the last complete line so a half-read final row cannot skew scoring.
        if let Some(last_newline) = sample_buf.iter().rposition(|&b| b == b'\n') {
            sample_buf.truncate(last_newline + 1);
        }

        // Detect the source encoding and transcode the sample to UTF-8 before scoring, so a
        // non-UTF-8 file (Windows-1252, Latin-1, ...) does not fail record parsing on its first
        // non-ASCII byte. The detected label is carried on the dialect for the ingestor to reuse.
        // ISO-2022-JP detection is denied, since it is only reliable in a browser context, and a
        // UTF-8 guess is allowed.
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        detector.feed(&sample_buf, true);
        let encoding = detector.guess(None, Utf8Detection::Allow);
        let encoding_name = encoding.name().to_string();
        let decoded = encoding.decode(&sample_buf).0;
        let sample = decoded.as_bytes();

        let terminator = if sample.windows(2).any(|w| w == b"\r\n") {
            LineTerminator::CrLf
        } else {
            LineTerminator::Lf
        };

        let mut best_dialect: Option<Dialect> = None;
        let mut best_table: Option<Table> = None;
        let mut best_score = f64::NEG_INFINITY;

        for &delimiter in &self.delimiters {
            for &quote in &self.quotes {
                // The full table keeps row 0, so it is both what the no-headers hypothesis scores and
                // what a header is judged against; the headers hypothesis scores the data-only table.
                let Some(full) = Self::build_dialect_table(sample, delimiter, quote, terminator, HeaderRow::Keep) else {
                    continue;
                };
                if full.num_columns <= 1 || full.num_rows == 0 {
                    continue;
                }

                let header_signal = Self::header_quality(&full);
                let full_rows = full.num_rows;

                let no_header_score = Self::score_table(&full);
                if no_header_score > best_score {
                    best_score = no_header_score;
                    best_dialect = Some(Self::dialect(delimiter, quote, false, terminator, &encoding_name));
                    best_table = Some(full);
                }

                if full_rows >= 2
                    && let Some(data) = Self::build_dialect_table(sample, delimiter, quote, terminator, HeaderRow::Skip)
                    && data.num_columns > 1
                    && data.num_rows > 0
                {
                    let header_score = Self::score_table(&data) + header_signal * 15.0;
                    if header_score > best_score {
                        best_score = header_score;
                        best_dialect = Some(Self::dialect(delimiter, quote, true, terminator, &encoding_name));
                        best_table = Some(data);
                    }
                }
            }
        }

        // Safety net: if the winner picked `has_headers = false` but row 0 looks
        // like a header, force headers on so the header row is not ingested.
        if let (Some(dialect), Some(table)) = (best_dialect.as_mut(), best_table.as_ref())
            && !dialect.has_headers
            && Self::looks_like_header_row(table)
        {
            dialect.has_headers = true;
        }

        best_dialect.ok_or(CsvInspectError::NoDialectDetected)
    }

    /// Reads a dialect sample large enough to score a header, bounded by a hard byte ceiling.
    ///
    /// A narrow CSV crosses [`MIN_SAMPLE_ROWS`] newlines well inside the soft target `sample_size`,
    /// so the loop stops at that target with a sample byte-identical to a single fixed read: the
    /// common path is unchanged. A file whose rows are wider than the soft target keeps reading
    /// until it holds enough complete rows to tell a header from data, and [`MAX_SAMPLE_BYTES`]
    /// caps a pathologically wide single row. Newlines are counted per chunk, never by rescanning
    /// the accumulated buffer, so the scan stays linear in the bytes read.
    ///
    /// # Errors
    ///
    /// Returns [`CsvInspectError::SampleRead`] if the underlying reader errors.
    fn read_sample<R: Read>(&self, mut reader: R) -> Result<Vec<u8>, CsvInspectError> {
        let mut sample = Vec::new();
        let mut chunk = vec![0u8; CHUNK];
        let mut newlines = 0usize;
        loop {
            let read = reader.read(&mut chunk).map_err(CsvInspectError::SampleRead)?;
            if read == 0 {
                break;
            }
            newlines += bytecount::count(&chunk[..read], b'\n');
            sample.extend_from_slice(&chunk[..read]);

            if sample.len() >= self.sample_size && newlines >= MIN_SAMPLE_ROWS {
                break;
            }
            if sample.len() >= MAX_SAMPLE_BYTES {
                break;
            }
        }
        Ok(sample)
    }

    fn looks_like_header_row(table: &Table) -> bool {
        if table.num_rows < 2 || table.column_types.is_empty() {
            return false;
        }

        // Row 0 must read as field labels rather than values on two counts: no cell carries a genuine
        // data value, and every cell is label-shaped (short and comma-free, so a multi-word textual
        // value is not mistaken for a header either).
        //
        // A boolean literal (`on`, `off`, `yes`, `no`, `true`, `false`, `y`, `n`) is a bare word that
        // is equally a valid column label, so a boolean-typed row-0 cell counts as a label here, not a
        // value. Without this a header column legitimately named `on` types as a boolean, row 0 is
        // read as data, the file is ingested headerless, and every name-keyed field reference misses
        // (the entity renders with a null id).
        let Some(first_row) = table.records.first() else {
            return false;
        };
        let row0_carries_no_value = table
            .column_types
            .iter()
            .all(|col| col.first().is_none_or(|t| matches!(t, DataType::Text | DataType::Empty | DataType::Boolean)));
        let row0_all_labels = !first_row.is_empty()
            && first_row.iter().all(|cell| {
                let cell = cell.trim();
                cell.is_empty() || is_field_label(cell)
            });
        if !row0_carries_no_value || !row0_all_labels {
            return false;
        }

        table
            .column_types
            .iter()
            .any(|col| col.iter().skip(1).any(|t| !matches!(t, DataType::Text | DataType::Empty)))
    }

    /// Parses `sample` under one dialect into a [`Table`], keeping or skipping row 0 per `header_row`.
    fn build_dialect_table(sample: &[u8], delimiter: u8, quote: Option<u8>, terminator: LineTerminator, header_row: HeaderRow) -> Option<Table> {
        let mut builder = ReaderBuilder::new();
        builder
            .delimiter(delimiter)
            .has_headers(header_row == HeaderRow::Skip)
            .terminator(terminator.as_csv());
        if let Some(quote) = quote {
            builder.quote(quote);
        } else {
            builder.quoting(false);
        }
        Self::build_table(builder.from_reader(sample))
    }

    /// Assembles a [`Dialect`] from the parameters a winning candidate was scored under.
    fn dialect(delimiter: u8, quote: Option<u8>, has_headers: bool, terminator: LineTerminator, encoding: &str) -> Dialect {
        Dialect {
            delimiter,
            quote_char: quote,
            has_headers,
            escape: None,
            terminator,
            encoding: encoding.to_string(),
        }
    }

    fn build_table<R: Read>(mut reader: Reader<R>) -> Option<Table> {
        let mut records = Vec::new();
        for result in reader.records() {
            match result {
                Ok(record) => records.push(record),
                Err(_) => return None,
            }
        }

        if records.is_empty() {
            return None;
        }

        let num_columns = records.first().map_or(0, StringRecord::len);
        let num_rows = records.len();
        let column_types = (0..num_columns)
            .map(|col| records.iter().map(|record| record.get(col).map_or(DataType::Empty, DataType::detect)).collect())
            .collect();

        Some(Table {
            records,
            column_types,
            num_columns,
            num_rows,
        })
    }

    /// Scores how coherent a parsed table is, independent of whether row 0 is a header. The header
    /// decision is made separately from [`Self::header_quality`], which the caller weighs in.
    fn score_table(table: &Table) -> f64 {
        let mut score = 0.0;
        score += Self::column_consistency(table) * 40.0;
        score += Self::type_uniformity(table) * 30.0;
        let col_bonus = to_f64(table.num_columns).min(20.0) / 20.0;
        score += col_bonus * 10.0;
        if table.num_rows <= 1 {
            score -= 20.0;
        }

        let empty_cols = table.column_types.iter().filter(|col| col.iter().all(|t| *t == DataType::Empty)).count();
        if table.num_columns > 0 {
            score -= (to_f64(empty_cols) / to_f64(table.num_columns)) * 15.0;
        }

        // A field still wrapped in matching quotes means the quote candidate under test left the
        // enclosing quotes in the data. The other scoring terms cannot see this: a quoted numeric
        // column reads as uniform `Text` whether or not the quotes were stripped, so quoting-off
        // would otherwise tie or beat quoting-on for every file with quoted numeric columns. This
        // penalty is what makes the correct quote character win.
        score -= Self::enclosed_quote_fraction(table) * 50.0;
        score
    }

    /// Fraction of non-empty parsed fields still enclosed in a matching pair of quote characters.
    fn enclosed_quote_fraction(table: &Table) -> f64 {
        let mut total = 0usize;
        let mut enclosed = 0usize;
        for record in &table.records {
            for field in record {
                let field = field.trim();
                if field.is_empty() {
                    continue;
                }
                total += 1;
                if is_enclosed_in_quotes(field) {
                    enclosed += 1;
                }
            }
        }

        if total == 0 { 0.0 } else { to_f64(enclosed) / to_f64(total) }
    }

    fn column_consistency(table: &Table) -> f64 {
        if table.records.is_empty() {
            return 0.0;
        }
        let expected = table.num_columns;
        let consistent = table.records.iter().filter(|r| r.len() == expected).count();
        to_f64(consistent) / to_f64(table.records.len())
    }

    fn type_uniformity(table: &Table) -> f64 {
        if table.column_types.is_empty() {
            return 0.0;
        }

        let mut total_uniformity = 0.0;
        for col_types in &table.column_types {
            let non_empty: Vec<&DataType> = col_types.iter().filter(|t| **t != DataType::Empty).collect();
            if non_empty.is_empty() {
                continue;
            }
            let mut type_counts: HashMap<&DataType, usize> = HashMap::new();
            for t in &non_empty {
                *type_counts.entry(t).or_insert(0) += 1;
            }
            let max_count = type_counts.values().max().copied().unwrap_or(0);
            total_uniformity += to_f64(max_count) / to_f64(non_empty.len());
        }

        total_uniformity / to_f64(table.column_types.len())
    }

    /// Scores how much row 0 looks like a header, as a signed value in `[-1, 1]`: positive is
    /// header-like, negative is data-like.
    ///
    /// The primary signal is a column-pattern violation. Each column's type is learned from the rows
    /// below row 0; row 0 is then header-like where it breaks a well-typed column (a text label over
    /// a numeric or boolean column) and data-like where it conforms to one (a number sitting in a
    /// numeric column). This is what separates a real header from a data row that merely happens to
    /// be mostly text: a data row's first cells still fit their columns' types.
    ///
    /// When no column carries a strong type (an all-text table), type gives no signal and the score
    /// falls back to row 0's shape: a header's cells are usually unique, non-blank textual labels.
    fn header_quality(table: &Table) -> f64 {
        let Some(first_row) = table.records.first() else {
            return 0.0;
        };
        if first_row.is_empty() || table.num_columns == 0 {
            return 0.0;
        }

        let mut evidence = 0.0;
        let mut typed_columns = 0usize;
        for column in &table.column_types {
            let Some(dominant) = dominant_typed_pattern(column.get(1..).unwrap_or(&[])) else {
                continue;
            };
            typed_columns += 1;
            match column.first().copied().unwrap_or(DataType::Empty) {
                // A text label sitting over a numeric or boolean column is header evidence.
                DataType::Text | DataType::Empty => evidence += 1.0,
                // A boolean literal (`on`, `off`, `yes`, `no`, `true`, `false`, `y`, `n`) is a bare
                // word that is equally a valid column label, so row 0 holding one over a boolean
                // column is genuinely ambiguous and must not be scored as a data row — a header named
                // `on` otherwise reads as data and the whole entity renders with a null id. It gives
                // no signal either way.
                DataType::Boolean => {}
                // Row 0 fits the column's own type, so it reads as one more data row.
                row0 if row0 == dominant => evidence -= 1.0,
                // Typed but a different type: inconclusive, contributes nothing.
                DataType::Integer
                | DataType::Float
                | DataType::Date
                | DataType::Time
                | DataType::DateTime
                | DataType::Email
                | DataType::Url
                | DataType::Phone
                | DataType::Currency
                | DataType::Percentage => {}
            }
        }

        if typed_columns > 0 {
            return evidence / to_f64(table.num_columns);
        }

        header_shape(first_row)
    }
}

fn to_f64(count: usize) -> f64 {
    u32::try_from(count).map_or_else(|_| f64::from(u32::MAX), f64::from)
}

/// The dominant type of a column's data cells, but only when that type is a strong, non-textual
/// pattern the header check can measure row 0 against.
///
/// `Empty` cells are ignored as missing values. The dominant type must cover at least four fifths of
/// the remaining cells, so a noisy or mixed column contributes no signal; a column that is dominantly
/// `Text` or has no typed cells returns `None`.
fn dominant_typed_pattern(cells: &[DataType]) -> Option<DataType> {
    let mut counts: HashMap<DataType, usize> = HashMap::new();
    let mut present = 0usize;
    for &cell in cells {
        if cell == DataType::Empty {
            continue;
        }
        present += 1;
        *counts.entry(cell).or_insert(0) += 1;
    }

    if present == 0 {
        return None;
    }

    let (dominant, count) = counts.into_iter().max_by_key(|&(_, count)| count)?;
    if matches!(dominant, DataType::Text | DataType::Empty) {
        return None;
    }

    (to_f64(count) / to_f64(present) >= 0.8).then_some(dominant)
}

/// The longest a cell can be and still read as a field label rather than a value.
const MAX_LABEL_LENGTH: usize = 40;

/// The most whitespace-separated words a field label carries; more reads as prose, i.e. a value.
const MAX_LABEL_WORDS: usize = 3;

/// A fallback header signal for an all-text table, where column types cannot separate a header from
/// a data row.
///
/// It rests on shape: a header row's cells look like field labels (short, distinct tokens such as
/// `name` or `iso_code`), whereas a data row carries real values. A single cell that reads as a
/// value rather than a label (one with a comma, a parenthesis, or several words) is taken as proof
/// that row 0 is data, so the score turns negative; an all-label row scores positive, more so when
/// its labels are distinct.
fn header_shape(first_row: &StringRecord) -> f64 {
    let mut seen = HashSet::new();
    let mut unique = true;
    let mut labels = 0usize;
    for field in first_row {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        if !is_field_label(field) {
            return -1.0;
        }
        labels += 1;
        if !seen.insert(field) {
            unique = false;
        }
    }

    if labels == 0 {
        return 0.0;
    }

    if unique { 1.0 } else { 0.5 }
}

/// Whether `cell` has the shape of a field label rather than a data value: short, free of the
/// commas, parentheses, and semicolons a value carries, and no more than a few words.
fn is_field_label(cell: &str) -> bool {
    cell.len() <= MAX_LABEL_LENGTH && !cell.contains([',', '(', ')', ';']) && cell.split_whitespace().count() <= MAX_LABEL_WORDS
}

/// Whether `field` begins and ends with the same quote character (`"` or `'`).
///
/// A leading apostrophe in ordinary text (`L'Aquila`) is not matched, because only a field that is
/// enclosed on both ends signals unstripped quoting rather than an incidental quote.
fn is_enclosed_in_quotes(field: &str) -> bool {
    let bytes = field.as_bytes();
    matches!(bytes, [first, .., last] if (*first == b'"' || *first == b'\'') && first == last)
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{CsvInspector, dialect::LineTerminator};

    #[test]
    fn a_comma_delimited_file_with_headers_is_detected() {
        let sample = "name,age,city\nAda,36,London\nGrace,45,New York\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert!(dialect.has_headers);
        assert_eq!(dialect.terminator, LineTerminator::Lf);
    }

    #[test]
    fn a_semicolon_delimited_file_is_detected() {
        let sample = "name;age;city\nAda;36;London\nGrace;45;New York\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, b';');
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(CsvInspector::new().inspect(&b""[..]).is_err());
    }

    #[test]
    fn a_headerless_file_whose_first_row_carries_typed_values_is_detected() {
        // Row 0 holds an integer id and float coordinates, so it is data, not a header. The detector
        // must not consume it as a header row (which would drop the first record and key the rest by
        // that row's values).
        let sample = "1,\"Goroka\",\"PG\",-6.081,145.391\n2,\"Madang\",\"PG\",-5.207,145.789\n3,\"Hagen\",\"PG\",-5.826,144.296\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert!(!dialect.has_headers);
    }

    #[test]
    fn an_all_text_file_whose_first_row_holds_values_is_not_read_as_headers() {
        // Every column is text, so type gives no signal; the first row's cells are real values (a
        // multi-word name, a code) rather than field labels, so it must be treated as data.
        let sample = "\"Bonaire, Saint Eustatius and Saba\",\"BQ\",\"\"\n\"Aruba\",\"AW\",\"AA\"\n\"Antigua and Barbuda\",\"AG\",\"AC\"\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert!(!dialect.has_headers);
    }

    #[test]
    fn an_all_text_file_whose_first_row_holds_field_labels_is_read_as_headers() {
        // Every column is text, but the first row's cells are short label tokens, so it is a header.
        let sample = "name,iso_code,dafif_code\nAruba,AW,AA\nAntigua and Barbuda,AG,AC\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert!(dialect.has_headers);
    }

    #[test]
    fn a_header_is_detected_when_the_first_data_row_exceeds_the_sample_size() {
        // The first data row is wider than the 32 KiB soft sample, so a single fixed read would see
        // only the header line, score it as headerless, and ingest it as data. Row-aware sampling
        // keeps reading until it holds enough complete rows to recognise the header. This mirrors
        // the TMDB credits file, whose first row (Avatar) is ~40 KiB of quoted JSON.
        let mut sample = String::from("id,title,cast\n1,Avatar,\"");
        sample.push_str(&"x".repeat(40 * 1024));
        sample.push_str("\"\n2,Titanic,\"short\"\n3,Aliens,\"short\"\n");
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert!(dialect.has_headers);
    }

    #[test]
    fn a_header_whose_label_spells_a_boolean_word_is_detected() {
        // A column legitimately named `on` (a camera on/off state) must not defeat header detection.
        // The header cell `on` matches the boolean-literal pattern, but it is a label, not a value:
        // read as data, the file ingests headerless, every name-keyed field reference (`device_code`,
        // `on`) misses, and the entity renders with a null id and no attributes.
        let sample = "device_code,timestamp,on\nCA001,2026-09-09T13:10:00Z,true\nCA002,2026-09-09T13:11:00Z,false\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert!(dialect.has_headers);
    }

    #[test]
    fn every_boolean_word_column_label_still_detects_a_header() {
        // The whole boolean vocabulary the type inspector recognises works as a header label.
        for label in ["on", "off", "yes", "no", "y", "n", "true", "false"] {
            let sample = format!("device_code,{label}\nCA001,true\nCA002,false\n");
            let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

            assert!(dialect.has_headers, "header with a `{label}` column was read as data");
        }
    }

    #[test]
    fn a_file_with_quoted_numeric_fields_detects_the_quote_character() {
        // Every value is double-quoted, including the numeric ones. Leaving quoting off would keep
        // the quotes in the data; the dialect must recover the quote character so they are stripped.
        let sample = "id;slots\n\"1\";\"30\"\n\"2\";\"24\"\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, b';');
        assert_eq!(dialect.quote_char, Some(b'"'));
        assert!(dialect.has_headers);
    }
}

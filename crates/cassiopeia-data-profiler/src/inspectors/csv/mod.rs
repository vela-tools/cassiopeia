use crate::{
    encoding_hint::EncodingHint,
    inspectors::csv::{
        dialect::{Dialect, LineTerminator},
        error::CsvInspectError,
        header::{continuation_rows, header_quality, looks_like_header_row},
        header_rows::HeaderRows,
        preamble::Preamble,
        table::Table,
        title::title_rows,
    },
};
use std::io::Read;

pub mod data_type;
pub mod dialect;
pub mod error;
mod header;
pub mod header_rows;
pub mod preamble;
mod table;
mod title;

/// Read granularity for [`CsvInspector::read_sample`].
const CHUNK: usize = 64 * 1024;

/// The fewest complete rows a sample must hold before header scoring can run: row 0 plus enough
/// data rows for both the header hypothesis (which needs two full rows) and per-column typing.
const MIN_SAMPLE_ROWS: usize = 5;

/// Hard ceiling on the dialect sample. It bounds the read, and the scoring work of the candidate
/// dialects, for a file whose rows are pathologically wide: a single row can exceed the soft
/// target on its own, so a fixed-size sample could hold no complete data row at all.
const MAX_SAMPLE_BYTES: usize = 4 * 1024 * 1024;

/// The most leading records a preamble may cover. It bounds the candidate preambles tried per
/// dialect; real title blocks above a table are a line or a few.
const MAX_PREAMBLE_RECORDS: usize = 10;

/// The fewest records a table below a non-empty preamble must hold: a header and a data row. A
/// shorter tail is more likely a coincidence of a wrong delimiter than a table.
const MIN_RECORDS_BELOW_PREAMBLE: usize = 2;

/// CSV dialect inspector.
///
/// The structural inspector for delimited text: it samples a payload and scores candidate dialects to
/// recover the delimiter, quote, preamble, header, and terminator a specific CSV file uses. It runs
/// whichever way the format was decided, as part of auto-detection and for a declared CSV alike.
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

    /// Inspects the dialect of `reader` by sampling and scoring candidate dialects, reading its
    /// character encoding with the context `encoding_hint` gives.
    ///
    /// # Errors
    ///
    /// Returns [`CsvInspectError`] when the sample cannot be read, is empty, or no candidate
    /// dialect produces a coherent table.
    pub fn inspect<R: Read>(&self, reader: R, encoding_hint: &EncodingHint) -> Result<Dialect, CsvInspectError> {
        let mut sample_buf = self.read_sample(reader)?;

        if sample_buf.is_empty() {
            return Err(CsvInspectError::EmptyInput);
        }

        // Truncate to the last complete line so a half-read final row cannot skew scoring.
        if let Some(last_newline) = sample_buf.iter().rposition(|&b| b == b'\n') {
            sample_buf.truncate(last_newline + 1);
        }

        // Transcode the sample to UTF-8 before scoring, so a non-UTF-8 file (Windows-1252, Latin-1,
        // ...) does not fail record parsing on its first non-ASCII byte. The encoding is carried on
        // the dialect for the ingestor to reuse.
        let encoding = encoding_hint.encoding_of(&sample_buf);
        let decoded = encoding.decode(&sample_buf).0;
        let sample = decoded.as_bytes();

        let terminator = if sample.windows(2).any(|w| w == b"\r\n") {
            LineTerminator::CrLf
        } else {
            LineTerminator::Lf
        };

        let mut best: Option<(Dialect, Table)> = None;
        let mut best_score = f64::NEG_INFINITY;

        for &delimiter in &self.delimiters {
            for &quote in &self.quotes {
                let headerless = Dialect {
                    delimiter,
                    quote_char: quote,
                    preamble: Preamble::NONE,
                    header_rows: HeaderRows::NONE,
                    escape: None,
                    terminator,
                    encoding: encoding.name().to_owned(),
                };
                // The full table keeps row 0, so it is both what the no-headers hypothesis scores and
                // what a header is judged against; the headers hypothesis scores the data-only table.
                let Some((headerless, full)) = Self::locate_table(sample, headerless) else {
                    continue;
                };
                let (headerless, full) = Self::below_titles(sample, headerless, full);

                let header_signal = header_quality(&full);
                let headed = Dialect {
                    header_rows: HeaderRows::ONE,
                    encoding: encoding.name().to_owned(),
                    ..headerless
                };
                let data = (full.num_rows >= 2).then(|| Table::parse(sample, &headed)).flatten().filter(Table::is_tabular);

                let no_header_score = full.score();
                if no_header_score > best_score {
                    best_score = no_header_score;
                    best = Some((headerless, full));
                }

                if let Some(data) = data {
                    let header_score = data.score() + header_signal * 15.0;
                    if header_score > best_score {
                        best_score = header_score;
                        best = Some((headed, data));
                    }
                }
            }
        }

        let (dialect, table) = best.ok_or(CsvInspectError::NoDialectDetected)?;
        Ok(Self::with_stacked_header(dialect, &table))
    }

    /// Settles how many records form the winning dialect's header, given the table it was scored on.
    ///
    /// A headerless winner whose row 0 still looks like a header gets one header record, so the
    /// header row is not ingested as data. A header is then extended by the records directly below
    /// it that continue it, such as aggregation and unit rows.
    fn with_stacked_header(mut dialect: Dialect, table: &Table) -> Dialect {
        let below_header = if dialect.header_rows.is_present() {
            table.records.as_slice()
        } else if looks_like_header_row(table) {
            table.records.get(1..).unwrap_or_default()
        } else {
            return dialect;
        };
        dialect.header_rows = HeaderRows::new(1 + continuation_rows(below_header));
        dialect
    }

    /// Finds where the table in `sample` starts under `dialect`'s delimiter and quote, returning the
    /// dialect with its preamble set and the headerless table below that preamble.
    ///
    /// A preamble is the run of leading records, such as a title line, whose field count differs
    /// from the table's. The reader is not flexible, so while such a record is still in front of the
    /// table the parse fails; the shortest preamble under which it succeeds is the one detected. A
    /// candidate preamble is only tried when every record in it is a different width from the record
    /// that would start the table, so a ragged row further down never turns the rows above it, the
    /// header among them, into a preamble.
    fn locate_table(sample: &[u8], mut dialect: Dialect) -> Option<(Dialect, Table)> {
        let widths: Vec<usize> = dialect
            .reader_builder()
            .flexible(true)
            .from_reader(sample)
            .records()
            .take(MAX_PREAMBLE_RECORDS + 1)
            .map_while(Result::ok)
            .map(|record| record.len())
            .collect();

        for (records, width) in widths.iter().enumerate() {
            if widths[..records].contains(width) {
                continue;
            }
            dialect.preamble = Preamble::new(records);
            if let Some(table) = Table::parse(sample, &dialect) {
                // Every record from here on shares one width, so a longer preamble could only drop
                // rows of the same table, never reveal a different one.
                let enough_rows = records == 0 || table.num_rows >= MIN_RECORDS_BELOW_PREAMBLE;
                return (table.is_tabular() && enough_rows).then_some((dialect, table));
            }
        }
        None
    }

    /// Extends `dialect`'s preamble over the title records at the top of `table` that are as wide as
    /// the table, returning the dialect and the headerless table below them. A dialect whose table
    /// starts with no such title, or whose preamble is already at [`MAX_PREAMBLE_RECORDS`], is
    /// returned unchanged.
    fn below_titles(sample: &[u8], mut dialect: Dialect, table: Table) -> (Dialect, Table) {
        let preamble = dialect.preamble;
        let titles = title_rows(&table).min(MAX_PREAMBLE_RECORDS.saturating_sub(preamble.records()));
        if titles == 0 {
            return (dialect, table);
        }

        dialect.preamble = Preamble::new(preamble.records() + titles);
        if let Some(below) = Table::parse(sample, &dialect) {
            return (dialect, below);
        }
        dialect.preamble = preamble;
        (dialect, table)
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
}

#[cfg(test)]
mod tests {
    use crate::{
        encoding_hint::EncodingHint,
        inspectors::csv::{CsvInspector, dialect::LineTerminator, header_rows::HeaderRows, preamble::Preamble},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_comma_delimited_file_with_headers_is_detected() {
        let sample = "name,age,city\nAda,36,London\nGrace,45,New York\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(dialect.terminator, LineTerminator::Lf);
    }

    #[test]
    fn a_semicolon_delimited_file_is_detected() {
        let sample = "name;age;city\nAda;36;London\nGrace;45;New York\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b';');
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(CsvInspector::new().inspect(&b""[..], &EncodingHint::NONE).is_err());
    }

    #[test]
    fn a_headerless_file_whose_first_row_carries_typed_values_is_detected() {
        // Row 0 holds an integer id and float coordinates, so it is data, not a header. The detector
        // must not consume it as a header row (which would drop the first record and key the rest by
        // that row's values).
        let sample = "1,\"Goroka\",\"PG\",-6.081,145.391\n2,\"Madang\",\"PG\",-5.207,145.789\n3,\"Hagen\",\"PG\",-5.826,144.296\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert_eq!(dialect.header_rows, HeaderRows::NONE);
    }

    #[test]
    fn an_all_text_file_whose_first_row_holds_values_is_not_read_as_headers() {
        // Every column is text, so type gives no signal; the first row's cells are real values (a
        // multi-word name, a code) rather than field labels, so it must be treated as data.
        let sample = "\"Bonaire, Saint Eustatius and Saba\",\"BQ\",\"\"\n\"Aruba\",\"AW\",\"AA\"\n\"Antigua and Barbuda\",\"AG\",\"AC\"\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert_eq!(dialect.header_rows, HeaderRows::NONE);
    }

    #[test]
    fn an_all_text_file_whose_first_row_holds_field_labels_is_read_as_headers() {
        // Every column is text, but the first row's cells are short label tokens, so it is a header.
        let sample = "name,iso_code,dafif_code\nAruba,AW,AA\nAntigua and Barbuda,AG,AC\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::ONE);
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
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b',');
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_header_whose_label_spells_a_boolean_word_is_detected() {
        // A column legitimately named `on` (a camera on/off state) must not defeat header detection.
        // The header cell `on` matches the boolean-literal pattern, but it is a label, not a value:
        // read as data, the file ingests headerless, every name-keyed field reference (`device_code`,
        // `on`) misses, and the entity renders with a null id and no attributes.
        let sample = "device_code,timestamp,on\nCA001,2026-09-09T13:10:00Z,true\nCA002,2026-09-09T13:11:00Z,false\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn every_boolean_word_column_label_still_detects_a_header() {
        // The whole boolean vocabulary the type inspector recognises works as a header label.
        for label in ["on", "off", "yes", "no", "y", "n", "true", "false"] {
            let sample = format!("device_code,{label}\nCA001,true\nCA002,false\n");
            let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

            assert_eq!(dialect.header_rows, HeaderRows::ONE, "header with a `{label}` column was read as data");
        }
    }

    #[test]
    fn a_file_with_quoted_numeric_fields_detects_the_quote_character() {
        // Every value is double-quoted, including the numeric ones. Leaving quoting off would keep
        // the quotes in the data; the dialect must recover the quote character so they are stripped.
        let sample = "id;slots\n\"1\";\"30\"\n\"2\";\"24\"\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b';');
        assert_eq!(dialect.quote_char, Some(b'"'));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    /// The reproduction from the issue: a three-field title line above a five-column table.
    const TITLED_AIR_QUALITY: &str = "Lumes;v2.10;25.10.26-08:00:00\nStation;Zeit-NO2;NO2;Zeit-O3;O3\nSTEF;25.10.2026, 08:00;12,45;25.10.2026, 08:00;54,55\nTAB;25.10.2026, 08:00;NE;25.10.2026, 08:00;NE\nAKA;25.10.2026, 08:00;9,80;25.10.2026, 08:00;40,10\n";

    #[test]
    fn a_title_line_above_the_header_is_detected_as_a_one_record_preamble() {
        let dialect = CsvInspector::new().inspect(TITLED_AIR_QUALITY.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b';');
        assert_eq!(dialect.quote_char, Some(b'"'));
        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(dialect.terminator, LineTerminator::Lf);
    }

    #[test]
    fn the_same_table_without_its_title_line_has_no_preamble() {
        let untitled = TITLED_AIR_QUALITY.split_once('\n').unwrap().1;
        let dialect = CsvInspector::new().inspect(untitled.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.delimiter, b';');
        assert_eq!(dialect.preamble, Preamble::NONE);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_quoted_title_spanning_two_lines_is_one_preamble_record() {
        // Five line feeds but four CSV records: the preamble is counted in records, so the header and
        // the first data row below the quoted line break stay whole.
        let sample = "\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\nAKA;9,80;40,10\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12,45", "54,55"]);
    }

    #[test]
    fn a_multi_line_title_block_is_detected_as_a_multi_record_preamble() {
        let sample = "Air quality;Vienna\nGenerated;25.10.26;08:00;CET;v2\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(2));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_blank_line_after_the_title_is_not_counted_as_a_preamble_record() {
        let sample = "Lumes;v2.10;25.10.26\n\nStation;NO2\nSTEF;12\nAKA;9\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2"]);
    }

    #[test]
    fn a_ragged_row_below_the_header_does_not_turn_the_header_into_a_preamble() {
        // Rows 0 to 2 share the table's width, so they cannot be a preamble; the file stays
        // undetectable rather than silently losing its header.
        let sample = "id;name\n1;alpha\n2;beta\n3;gamma;extra\n4;delta\n5;epsilon\n";

        assert!(CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).is_err());
    }

    #[test]
    fn a_stacked_name_aggregation_and_unit_header_is_detected_as_three_header_rows() {
        // The shape of Vienna's live air-quality table: a title line, then a name row, an
        // aggregation row, and a unit row over the data. Every data cell is text to the type
        // detector (decimal commas, `NE` for missing), so the extra header rows are recognised by
        // their shape: all labels, and blank in the station column every data row fills.
        let sample = "Lumes;v2.10;30.10.26-10:00:00\n;Zeit-NO2;NO2;Zeit-PM10;PM10;PM10\n;;HMW;;MW24;HMW\n;MESZ;µg/m³;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;30.10.2026, 10:00;NE;NE\nTAB;30.10.2026, 10:00;NE;30.10.2026, 10:00;14,03;14,44\nKE;30.10.2026, 10:00;15,17;30.10.2026, 10:00;12,49;18,80\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::new(3));
        assert_eq!(
            reader.headers().unwrap(),
            vec!["", "Zeit-NO2 MESZ", "NO2 HMW µg/m³", "Zeit-PM10 MESZ", "PM10 MW24 µg/m³", "PM10 HMW µg/m³"]
        );
        assert_eq!(reader.records().next().unwrap().unwrap()[0], *"STEF");
    }

    #[test]
    fn a_title_padded_to_the_table_width_is_detected_as_a_preamble() {
        // A spreadsheet export pads every row to the table's width, so the title has as many
        // fields as the header and the field-count check alone cannot see it.
        let sample = "Air quality report;;\nStation;NO2;O3\nSTEF;12,45;NE\nAKA;9,80;40,10\nTAB;NE;38,20\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
    }

    #[test]
    fn a_dense_title_as_wide_as_a_numeric_table_is_detected_as_a_preamble() {
        let sample = "Lumes;v2.10;25.10.26\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12", "54"]);
    }

    #[test]
    fn a_padded_title_above_a_stacked_header_leaves_the_stacked_header_whole() {
        let sample = "Lumes Wien;;;\n;Zeit-NO2;NO2;NO2\n;;HMW;MW24\n;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;12,1\nTAB;30.10.2026, 10:00;NE;14,03\nKE;30.10.2026, 10:00;15,17;12,49\n";
        let dialect = CsvInspector::new().inspect(sample.as_bytes(), &EncodingHint::NONE).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::new(3));
        assert_eq!(reader.headers().unwrap(), vec!["", "Zeit-NO2 MESZ", "NO2 HMW µg/m³", "NO2 MW24 µg/m³"]);
    }
}

use crate::{
    content::Content,
    inspectors::csv::{
        error::CsvInspectError,
        escape_evidence::backs_escape,
        header::{continuation_rows, header_quality, looks_like_header_row},
        reading::Reading,
        table::Table,
        table_shape::TableShape,
        title::title_rows,
    },
};
use cassiopeia_csv_dialect::{
    character_encoding::CharacterEncoding,
    dialect::Dialect,
    dialect_byte::DialectByte,
    escape::Escape,
    header_rows::HeaderRows,
    header_statement::HeaderStatement,
    line_terminator::LineTerminator,
    preamble::Preamble,
    quote::Quote,
    statement::DialectStatement,
    value_notation::ValueNotation,
};
use encoding_rs::Encoding;

pub mod data_type;
pub mod error;
mod escape_evidence;
mod header;
mod reading;
mod table;
mod table_shape;
mod title;

/// Scan granularity for [`CsvInspector::sample_of`].
const CHUNK: usize = 64 * 1024;

/// The fewest lines a sample must hold before header scoring can run: row 0 plus enough data rows
/// for both the header hypothesis (which needs two full rows) and per-column typing.
///
/// Lines stand in for records here because where a record ends depends on the quote, which is not
/// detected yet. A quoted field with a line break makes a record span several lines, so a sample of
/// very wide multi-line records can hold fewer records than this; it is then scored on fewer rows,
/// and is still cut at a record boundary.
const MIN_SAMPLE_LINES: usize = 5;

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

/// The soft target size of the dialect sample; see [`CsvInspector::sample_of`].
const SAMPLE_SIZE: usize = 32 * 1024;

/// The delimiters tried when none is stated.
const CANDIDATE_DELIMITERS: [DialectByte; 5] = [
    DialectByte::COMMA,
    DialectByte::TAB,
    DialectByte::SEMICOLON,
    DialectByte::PIPE,
    DialectByte::SPACE,
];

/// The quoting tried when none is stated.
const CANDIDATE_QUOTES: [Quote; 3] = [Quote::Char(DialectByte::DOUBLE_QUOTE), Quote::Char(DialectByte::SINGLE_QUOTE), Quote::Disabled];

/// The escape byte tried, in front of the doubling RFC 4180 states, when none is stated and the
/// sample backs it; see [`backs_escape`].
const CANDIDATE_ESCAPE: DialectByte = DialectByte::BACKSLASH;

/// CSV dialect inspector.
///
/// The structural inspector for delimited text: it samples a payload and scores candidate dialects to
/// recover the delimiter, quote, escape, preamble, header, and terminator a specific CSV file uses. It runs
/// whichever way the format was decided, as part of auto-detection and for a declared CSV alike.
///
/// A member its [`DialectStatement`] states is taken as stated, and only the members left out are
/// detected; a statement of every structural member is the dialect, and nothing is detected.
pub struct CsvInspector<'s> {
    statement: &'s DialectStatement,
    sample_size: usize,
}

/// Whether the dialect sample is the whole content or a leading part cut from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleExtent {
    /// The sample is the whole content, so its last record is the file's last record.
    Whole,
    /// The sample was cut from the content, so its last record may be cut short.
    Cut,
}

impl<'s> CsvInspector<'s> {
    /// Creates an inspector that takes what `statement` states and detects the rest.
    #[must_use]
    pub const fn new(statement: &'s DialectStatement) -> CsvInspector<'s> {
        CsvInspector {
            statement,
            sample_size: SAMPLE_SIZE,
        }
    }

    /// Inspects the dialect of `content` by sampling and scoring candidate dialects. Its character
    /// encoding is read from the whole content, with the context its source supplied.
    ///
    /// # Errors
    ///
    /// Returns [`CsvInspectError`] when the content is empty or no candidate dialect produces a
    /// coherent table.
    ///
    /// A table of several columns is looked for first; a file that no candidate splits into
    /// columns, such as a plain list of names, is then read as a table of one column.
    ///
    /// A byte order mark names the encoding even over a stated one. Every decoder of the web's
    /// encodings sniffs the mark ahead of any label (WHATWG Encoding Standard, `decode`), the
    /// ingestor's included, so the file is read in the mark's encoding whatever the manifest says,
    /// and the dialect reports the encoding the file is actually read in.
    pub fn inspect(&self, content: &Content<'_>) -> Result<Dialect, CsvInspectError> {
        let bytes = content.bytes();
        let marked = Encoding::for_bom(bytes).map(|(encoding, _)| CharacterEncoding::new(encoding));
        if let Some(dialect) = self.statement.whole_dialect() {
            return Ok(Dialect {
                encoding: marked.unwrap_or(dialect.encoding),
                ..dialect
            });
        }

        if bytes.is_empty() {
            return Err(CsvInspectError::EmptyInput);
        }

        // Transcode the sample to UTF-8 before scoring, so a non-UTF-8 file (Windows-1252, Latin-1,
        // ...) does not fail record parsing on its first non-ASCII byte. The encoding is carried on
        // the dialect for the ingestor to reuse.
        let encoding = marked
            .or_else(|| self.statement.encoding())
            .unwrap_or_else(|| CharacterEncoding::new(content.encoding_hint().encoding_of(bytes)));
        let (raw_sample, extent) = self.sample_of(bytes);
        let decoded = encoding.encoding().decode(raw_sample).0;
        let sample = decoded.as_bytes();
        let terminator = self.statement.terminator().unwrap_or_else(|| Self::terminator_of(sample));
        let best = self
            .best_of(sample, extent, terminator, encoding, TableShape::MultiColumn)
            .or_else(|| self.best_of(sample, extent, terminator, encoding, TableShape::SingleColumn));

        let Reading { mut dialect, table, .. } = best.ok_or(CsvInspectError::NoDialectDetected)?;
        if !matches!(self.statement.header(), Some(HeaderStatement::Rows(_))) {
            dialect = Self::with_stacked_header(dialect, &table);
        }
        // The dialect owns its notation, while the statement stays the manifest's.
        dialect.notation = self.statement.notation().clone();
        Ok(dialect)
    }

    /// The best reading of `sample`, whose lines end with `terminator` and which was decoded from
    /// `encoding`, as a table of `shape`, over every candidate delimiter, quote, and escape.
    fn best_of(&self, sample: &[u8], extent: SampleExtent, terminator: LineTerminator, encoding: CharacterEncoding, shape: TableShape) -> Option<Reading> {
        self.delimiters()
            .into_iter()
            .flat_map(|delimiter| self.quotes().into_iter().map(move |quote| (delimiter, quote)))
            .filter_map(|(delimiter, quote)| {
                // The first escape whose candidate reads a table is the one scored, so an escape the
                // sample backs is tried alone and doubling only when that escape reads no table.
                self.escapes(sample, delimiter, quote).into_iter().find_map(|escape| {
                    let candidate = Dialect {
                        delimiter,
                        quote_char: quote,
                        preamble: Preamble::NONE,
                        header_rows: HeaderRows::NONE,
                        escape,
                        terminator,
                        encoding,
                        notation: ValueNotation::PLAIN,
                    };
                    // A quote and its escape decide which line breaks end a record, so a cut sample
                    // is trimmed to its complete records under each candidate, never at a line.
                    let records = match extent {
                        SampleExtent::Whole => sample,
                        SampleExtent::Cut => candidate.complete_records(sample),
                    };
                    self.best_reading(records, candidate, shape)
                })
            })
            .reduce(|best, reading| best.or_better(Some(reading)))
    }

    /// The delimiters to try: the stated one, or every candidate.
    fn delimiters(&self) -> Vec<DialectByte> {
        self.statement
            .delimiter()
            .map_or_else(|| CANDIDATE_DELIMITERS.to_vec(), |delimiter| vec![delimiter])
    }

    /// The quoting to try: the stated one, or every candidate. A stated escape needs a quote to act
    /// in, so under one, disabled quoting is no candidate.
    fn quotes(&self) -> Vec<Quote> {
        self.statement.quote().map_or_else(
            || {
                CANDIDATE_QUOTES
                    .into_iter()
                    .filter(|quote| match (quote, self.statement.escape()) {
                        (Quote::Disabled, Some(_)) => false,
                        (Quote::Disabled, None) | (Quote::Char(_), Some(_) | None) => true,
                    })
                    .collect()
            },
            |quote| vec![quote],
        )
    }

    /// The escapes to try under `quote`, in order of preference: the stated one, or the candidate
    /// escape when `sample` backs it and doubling after it, or doubling alone.
    ///
    /// Error-free parsing cannot choose between doubling and an escape byte, since `csv_core` reads
    /// a file of either kind without error under both; [`backs_escape`] holds the rule that does.
    /// Doubling is RFC 4180's escape, so the escape byte is tried only on the sample's evidence,
    /// which keeps the candidates scored for an RFC 4180 file what they were without it.
    fn escapes(&self, sample: &[u8], delimiter: DialectByte, quote: Quote) -> Vec<Escape> {
        match (quote, self.statement.escape()) {
            (Quote::Char(_), Some(escape)) => vec![escape],
            (Quote::Char(quote), None) if backs_escape(sample, delimiter, quote, CANDIDATE_ESCAPE) => {
                vec![Escape::Char(CANDIDATE_ESCAPE), Escape::Doubled]
            }
            // An escape has no effect without a quote, so disabled quoting carries the default.
            (Quote::Disabled, Some(_) | None) | (Quote::Char(_), None) => vec![Escape::Doubled],
        }
    }

    /// The line terminator of `sample`: CRLF as soon as one line ends with it, a lone CR when the
    /// sample has more carriage returns than line feeds, and LF otherwise.
    ///
    /// The quote is not detected yet, so a line break inside a quoted field cannot be told from one
    /// that ends a record; the more frequent byte is taken as the terminator, and the other as such a
    /// quoted break. A CR-terminated file with a line feed in a quoted cell is still read by CR.
    fn terminator_of(sample: &[u8]) -> LineTerminator {
        if sample.windows(2).any(|w| w == b"\r\n") {
            LineTerminator::Crlf
        } else if bytecount::count(sample, b'\r') > bytecount::count(sample, b'\n') {
            LineTerminator::Cr
        } else {
            LineTerminator::Lf
        }
    }

    /// The best reading of `sample` under `candidate`'s delimiter and quote: where its table starts
    /// and whether it has a header, each as stated or, left out, as scored.
    ///
    /// The full table keeps row 0, so it is both what the no-headers hypothesis scores and what a
    /// header is judged against; the headers hypothesis scores the data-only table. A stated header
    /// leaves one hypothesis, scored on the table alone.
    fn best_reading(&self, sample: &[u8], candidate: Dialect, shape: TableShape) -> Option<Reading> {
        let (headerless, full) = self.table_start(sample, candidate, shape)?;
        match self.statement.header() {
            None => {
                let header_signal = header_quality(&full);
                let headed = Self::headed(sample, &headerless, HeaderRows::ONE, &full, shape).map(|(dialect, data)| Reading {
                    score: data.score() + header_signal * 15.0,
                    dialect,
                    table: data,
                });
                let unheaded = Reading {
                    score: full.score(),
                    dialect: headerless,
                    table: full,
                };
                Some(unheaded.or_better(headed))
            }
            Some(HeaderStatement::Present) => Self::headed_reading(sample, &headerless, HeaderRows::ONE, &full, shape),
            Some(HeaderStatement::Rows(rows)) if rows.is_present() => Self::headed_reading(sample, &headerless, rows, &full, shape),
            Some(HeaderStatement::Rows(_)) => Some(Reading {
                score: full.score(),
                dialect: headerless,
                table: full,
            }),
        }
    }

    /// The reading of `sample` with a header of `header_rows` records, scored on its data alone.
    fn headed_reading(sample: &[u8], headerless: &Dialect, header_rows: HeaderRows, full: &Table, shape: TableShape) -> Option<Reading> {
        Self::headed(sample, headerless, header_rows, full, shape).map(|(dialect, data)| Reading {
            score: data.score(),
            dialect,
            table: data,
        })
    }

    /// `headerless` with a header of `header_rows` records, and the data-only table below that
    /// header, when the full table holds a header and a data row and the data is tabular in `shape`.
    fn headed(sample: &[u8], headerless: &Dialect, header_rows: HeaderRows, full: &Table, shape: TableShape) -> Option<(Dialect, Table)> {
        let headed = Dialect {
            header_rows,
            notation: ValueNotation::PLAIN,
            ..*headerless
        };
        (full.num_rows >= 2)
            .then(|| Table::parse(sample, &headed))
            .flatten()
            .filter(|data| data.is_tabular(shape))
            .map(|data| (headed, data))
    }

    /// Where the table in `sample` starts under `candidate`: below the stated preamble, or below the
    /// preamble and titles detected, with the headerless table of `shape` from there.
    fn table_start(&self, sample: &[u8], candidate: Dialect, shape: TableShape) -> Option<(Dialect, Table)> {
        if let Some(preamble) = self.statement.skip_rows() {
            let dialect = Dialect { preamble, ..candidate };
            let table = Table::parse(sample, &dialect).filter(|table| table.is_tabular(shape))?;
            return Some((dialect, table));
        }
        let (dialect, table) = Self::locate_table(sample, candidate, shape)?;
        Some(Self::below_titles(sample, dialect, table))
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

    /// Finds where the table of `shape` in `sample` starts under `dialect`'s delimiter and quote,
    /// returning the dialect with its preamble set and the headerless table below that preamble.
    ///
    /// A preamble is the run of leading records, such as a title line, whose field count differs
    /// from the table's. The reader is not flexible, so while such a record is still in front of the
    /// table the parse fails; the shortest preamble under which it succeeds is the one detected. A
    /// candidate preamble is only tried when every record in it is a different width from the record
    /// that would start the table, so a ragged row further down never turns the rows above it, the
    /// header among them, into a preamble.
    fn locate_table(sample: &[u8], mut dialect: Dialect, shape: TableShape) -> Option<(Dialect, Table)> {
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
                return (table.is_tabular(shape) && enough_rows).then_some((dialect, table));
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

    /// The leading bytes of `bytes` the dialect is scored on, large enough to score a header and
    /// bounded by a hard byte ceiling, and whether they are all of `bytes`.
    ///
    /// A narrow CSV crosses [`MIN_SAMPLE_LINES`] line ends well inside the soft target `sample_size`,
    /// so the sample stops at that target. A file whose rows are wider than the soft target keeps
    /// sampling until it holds enough lines to tell a header from data, and [`MAX_SAMPLE_BYTES`]
    /// caps a pathologically wide single row. Line ends are counted per chunk, never by rescanning
    /// the sample, so the scan stays linear in the bytes sampled. The terminator is not detected
    /// yet, so a chunk's line ends are its line feeds or its carriage returns, whichever are more:
    /// a CRLF line holds one of each, and a file whose lines end with a lone CR has no line feed.
    ///
    /// A cut sample ends wherever its last chunk does, even inside a record or a character; each
    /// candidate dialect trims it to its complete records with [`Dialect::complete_records`].
    fn sample_of<'a>(&self, bytes: &'a [u8]) -> (&'a [u8], SampleExtent) {
        let mut end = 0usize;
        let mut line_ends = 0usize;
        for chunk in bytes.chunks(CHUNK) {
            line_ends += bytecount::count(chunk, b'\n').max(bytecount::count(chunk, b'\r'));
            end += chunk.len();
            if (end >= self.sample_size && line_ends >= MIN_SAMPLE_LINES) || end >= MAX_SAMPLE_BYTES {
                break;
            }
        }
        let extent = if end < bytes.len() { SampleExtent::Cut } else { SampleExtent::Whole };
        (&bytes[..end], extent)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        content::Content,
        inspectors::csv::{CHUNK, CsvInspector},
    };
    use cassiopeia_csv_dialect::{
        decimal_separator::DecimalSeparator,
        dialect_byte::DialectByte,
        escape::Escape,
        header_rows::HeaderRows,
        line_terminator::LineTerminator,
        preamble::Preamble,
        quote::Quote,
        statement::DialectStatement,
    };
    use encoding_rs::{UTF_8, UTF_16LE, WINDOWS_1252};
    use pretty_assertions::assert_eq;
    use std::fmt::Write;

    #[test]
    fn a_file_whose_lines_end_with_a_lone_carriage_return_is_detected_with_a_cr_terminator() {
        let sample = "name,age,city\rAda,36,London\rGrace,45,New York\r";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(dialect.terminator, LineTerminator::Cr);
    }

    #[test]
    fn a_carriage_return_inside_a_file_of_line_feeds_leaves_the_terminator_lf() {
        let sample = "name,note\nAda,\"one\rtwo\"\nGrace,three\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.terminator, LineTerminator::Lf);
    }

    #[test]
    fn a_line_feed_inside_a_file_of_lone_carriage_returns_leaves_the_terminator_cr() {
        let sample = "id;note\r1;\"one\ntwo\"\r2;three\r3;four\r";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.terminator, LineTerminator::Cr);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(reader.headers().unwrap(), vec!["id", "note"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", "one\ntwo"]);
    }

    #[test]
    fn a_single_column_file_is_read_as_one_column_under_a_header() {
        let sample = "name\nAda\nGrace\nLinus\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.preamble, Preamble::NONE);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_single_column_of_numbers_takes_its_label_as_the_header() {
        let sample = "count\n1\n2\n3\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_file_that_splits_into_columns_is_never_read_as_a_single_column() {
        // Every row is one wide under the comma, and its text is uniform there; only the semicolon
        // splits it, into columns of mixed types that score lower than the single column would.
        let sample = "id;value\n1;a\n2;3\nx;4\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
    }

    #[test]
    fn a_utf_16le_file_without_a_byte_order_mark_is_detected_in_utf_16le() {
        let bytes: Vec<u8> = "id,name\n1,Čapek\n2,Grace\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(&bytes)).unwrap();

        assert_eq!(dialect.character_encoding(), UTF_16LE);
        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_comma_delimited_file_with_headers_is_detected() {
        let sample = "name,age,city\nAda,36,London\nGrace,45,New York\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(dialect.terminator, LineTerminator::Lf);
    }

    #[test]
    fn a_semicolon_delimited_file_is_detected() {
        let sample = "name;age;city\nAda;36;London\nGrace;45;New York\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(b"")).is_err());
    }

    #[test]
    fn a_headerless_file_whose_first_row_carries_typed_values_is_detected() {
        // Row 0 holds an integer id and float coordinates, so it is data, not a header. The detector
        // must not consume it as a header row (which would drop the first record and key the rest by
        // that row's values).
        let sample = "1,\"Goroka\",\"PG\",-6.081,145.391\n2,\"Madang\",\"PG\",-5.207,145.789\n3,\"Hagen\",\"PG\",-5.826,144.296\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.header_rows, HeaderRows::NONE);
    }

    #[test]
    fn an_all_text_file_whose_first_row_holds_values_is_not_read_as_headers() {
        // Every column is text, so type gives no signal; the first row's cells are real values (a
        // multi-word name, a code) rather than field labels, so it must be treated as data.
        let sample = "\"Bonaire, Saint Eustatius and Saba\",\"BQ\",\"\"\n\"Aruba\",\"AW\",\"AA\"\n\"Antigua and Barbuda\",\"AG\",\"AC\"\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.header_rows, HeaderRows::NONE);
    }

    #[test]
    fn an_all_text_file_whose_first_row_holds_field_labels_is_read_as_headers() {
        // Every column is text, but the first row's cells are short label tokens, so it is a header.
        let sample = "name,iso_code,dafif_code\nAruba,AW,AA\nAntigua and Barbuda,AG,AC\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_header_is_detected_when_the_first_data_row_exceeds_the_sample_size() {
        // The first data row is wider than the 32 KiB soft sample, so a single fixed read would see
        // only the header line, score it as headerless, and ingest it as data. Line-aware sampling
        // keeps reading until it holds enough lines to recognise the header. This mirrors
        // the TMDB credits file, whose first row (Avatar) is ~40 KiB of quoted JSON.
        let mut sample = String::from("id,title,cast\n1,Avatar,\"");
        sample.push_str(&"x".repeat(40 * 1024));
        sample.push_str("\"\n2,Titanic,\"short\"\n3,Aliens,\"short\"\n");
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_sample_cut_inside_a_quoted_line_break_keeps_the_quote_and_the_header() {
        // Every note holds a line break, and the file is longer than one sample chunk, so the sample
        // is cut inside the second line of a note. Cut at its last line, the sample would end in half
        // a record, the double quote would fail to parse it, and the single quote would win with the
        // header taken for a preamble.
        let mut sample = String::from("station_identifier;observation_note;value\n");
        let mut row = 0;
        while sample.len() < CHUNK + 4096 {
            writeln!(sample, "{row};\"first line of a long note\nsecond line of a long note\";{row}.5").unwrap();
            row += 1;
        }
        let before_cut = &sample[..CHUNK];
        assert!(before_cut.rsplit('\n').next().unwrap().starts_with("second line"));

        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(dialect.quote_char, Quote::Char(DialectByte::DOUBLE_QUOTE));
        assert_eq!(dialect.preamble, Preamble::NONE);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_header_whose_label_spells_a_boolean_word_is_detected() {
        // A column legitimately named `on` (a camera on/off state) must not defeat header detection.
        // The header cell `on` matches the boolean-literal pattern, but it is a label, not a value:
        // read as data, the file ingests headerless, every name-keyed field reference (`device_code`,
        // `on`) misses, and the entity renders with a null id and no attributes.
        let sample = "device_code,timestamp,on\nCA001,2026-09-09T13:10:00Z,true\nCA002,2026-09-09T13:11:00Z,false\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn every_boolean_word_column_label_still_detects_a_header() {
        // The whole boolean vocabulary the type inspector recognises works as a header label.
        for label in ["on", "off", "yes", "no", "y", "n", "true", "false"] {
            let sample = format!("device_code,{label}\nCA001,true\nCA002,false\n");
            let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

            assert_eq!(dialect.header_rows, HeaderRows::ONE, "header with a `{label}` column was read as data");
        }
    }

    #[test]
    fn a_file_with_quoted_numeric_fields_detects_the_quote_character() {
        // Every value is double-quoted, including the numeric ones. Leaving quoting off would keep
        // the quotes in the data; the dialect must recover the quote character so they are stripped.
        let sample = "id;slots\n\"1\";\"30\"\n\"2\";\"24\"\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(dialect.quote_char, Quote::Char(DialectByte::DOUBLE_QUOTE));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    /// The reproduction from the issue: a three-field title line above a five-column table.
    const TITLED_AIR_QUALITY: &str = "Lumes;v2.10;25.10.26-08:00:00\nStation;Zeit-NO2;NO2;Zeit-O3;O3\nSTEF;25.10.2026, 08:00;12,45;25.10.2026, 08:00;54,55\nTAB;25.10.2026, 08:00;NE;25.10.2026, 08:00;NE\nAKA;25.10.2026, 08:00;9,80;25.10.2026, 08:00;40,10\n";

    #[test]
    fn a_title_line_above_the_header_is_detected_as_a_one_record_preamble() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(TITLED_AIR_QUALITY.as_bytes()))
            .unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(dialect.quote_char, Quote::Char(DialectByte::DOUBLE_QUOTE));
        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(dialect.terminator, LineTerminator::Lf);
    }

    #[test]
    fn the_same_table_without_its_title_line_has_no_preamble() {
        let untitled = TITLED_AIR_QUALITY.split_once('\n').unwrap().1;
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(untitled.as_bytes()))
            .unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(dialect.preamble, Preamble::NONE);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_quoted_title_spanning_two_lines_is_one_preamble_record() {
        // Five line feeds but four CSV records: the preamble is counted in records, so the header and
        // the first data row below the quoted line break stay whole.
        let sample = "\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\nAKA;9,80;40,10\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12,45", "54,55"]);
    }

    #[test]
    fn a_title_above_an_all_text_table_of_one_data_row_is_a_preamble_over_a_header() {
        let sample = "Lumes;v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12,45", "54,55"]);
    }

    #[test]
    fn a_quoted_two_line_title_above_one_data_row_is_one_preamble_record_over_a_header() {
        let sample = "\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2;O3\nSTEF;12,45;54,55\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12,45", "54,55"]);
    }

    #[test]
    fn an_all_text_table_of_one_data_row_is_read_under_its_header() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(b"Station;NO2;O3\nSTEF;12,45;54,55\n"))
            .unwrap();

        assert_eq!(dialect.preamble, Preamble::NONE);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_numeric_table_of_two_rows_stays_headerless() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(b"1;2;3\n4;5;6\n")).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::NONE);
    }

    #[test]
    fn a_multi_line_title_block_is_detected_as_a_multi_record_preamble() {
        let sample = "Air quality;Vienna\nGenerated;25.10.26;08:00;CET;v2\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(2));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_blank_line_after_the_title_is_not_counted_as_a_preamble_record() {
        let sample = "Lumes;v2.10;25.10.26\n\nStation;NO2\nSTEF;12\nAKA;9\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2"]);
    }

    #[test]
    fn a_ragged_row_below_the_header_does_not_turn_the_header_into_a_preamble() {
        // Rows 0 to 2 share the table's width, so they cannot be a preamble; the ragged row is
        // skipped and the header stays the header.
        let sample = "id;name\n1;alpha\n2;beta\n3;gamma;extra\n4;delta\n5;epsilon\n";

        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(dialect.preamble, Preamble::NONE);
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_stacked_name_aggregation_and_unit_header_is_detected_as_three_header_rows() {
        // The shape of Vienna's live air-quality table: a title line, then a name row, an
        // aggregation row, and a unit row over the data. Every data cell is text to the type
        // detector (decimal commas, `NE` for missing), so the extra header rows are recognised by
        // their shape: all labels, and blank in the station column every data row fills.
        let sample = "Lumes;v2.10;30.10.26-10:00:00\n;Zeit-NO2;NO2;Zeit-PM10;PM10;PM10\n;;HMW;;MW24;HMW\n;MESZ;µg/m³;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;30.10.2026, 10:00;NE;NE\nTAB;30.10.2026, 10:00;NE;30.10.2026, 10:00;14,03;14,44\nKE;30.10.2026, 10:00;15,17;30.10.2026, 10:00;12,49;18,80\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
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
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::ONE);
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
    }

    #[test]
    fn a_dense_title_as_wide_as_a_numeric_table_is_detected_as_a_preamble() {
        let sample = "Lumes;v2.10;25.10.26\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2", "O3"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12", "54"]);
    }

    #[test]
    fn a_padded_title_above_a_stacked_header_leaves_the_stacked_header_whole() {
        let sample = "Lumes Wien;;;\n;Zeit-NO2;NO2;NO2\n;;HMW;MW24\n;MESZ;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;12,1\nTAB;30.10.2026, 10:00;NE;14,03\nKE;30.10.2026, 10:00;15,17;12,49\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::new(3));
        assert_eq!(reader.headers().unwrap(), vec!["", "Zeit-NO2 MESZ", "NO2 HMW µg/m³", "NO2 MW24 µg/m³"]);
    }

    /// A table whose rows are plain ASCII well past the dialect sample, then one row carrying `name`.
    fn ascii_rows_then(name: &[u8]) -> Vec<u8> {
        let mut bytes = b"id;name\n".to_vec();
        (0..4000).for_each(|id| bytes.extend_from_slice(format!("{id};Station\n").as_bytes()));
        bytes.extend_from_slice(b"4000;");
        bytes.extend_from_slice(name);
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn a_latin_1_name_past_the_sample_settles_the_encoding() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(&ascii_rows_then(b"M\xfcller")))
            .unwrap();

        assert_eq!(dialect.encoding.to_string(), "windows-1252");
    }

    #[test]
    fn a_utf_8_name_past_the_sample_settles_the_encoding() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(&ascii_rows_then("Müller".as_bytes())))
            .unwrap();

        assert_eq!(dialect.encoding.to_string(), "UTF-8");
    }

    fn stated(statement: serde_json::Value) -> DialectStatement {
        serde_json::from_value(statement).unwrap()
    }

    #[test]
    fn a_byte_order_mark_names_the_encoding_over_a_stated_one() {
        let statement = stated(serde_json::json!({"encoding": "windows-1252"}));
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend("id,name\n1,Čapek\n2,Grace\n".encode_utf16().flat_map(u16::to_le_bytes));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(&bytes)).unwrap();

        assert_eq!(dialect.character_encoding(), UTF_16LE);
        assert_eq!(dialect.delimiter, DialectByte::COMMA);
    }

    #[test]
    fn a_byte_order_mark_names_the_encoding_of_a_wholly_stated_dialect() {
        let statement = stated(serde_json::json!({
            "delimiter": ",",
            "quote": "\"",
            "skipRows": 0,
            "headerRows": 1,
            "terminator": "lf",
            "encoding": "windows-1252",
        }));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(b"\xef\xbb\xbfid,name\n1,Ada\n")).unwrap();

        assert_eq!(dialect.character_encoding(), UTF_8);
    }

    #[test]
    fn a_stated_encoding_holds_for_a_file_without_a_byte_order_mark() {
        let statement = stated(serde_json::json!({"encoding": "windows-1252"}));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(b"id,name\n1,Caf\xe9\n2,Grace\n")).unwrap();

        assert_eq!(dialect.character_encoding(), WINDOWS_1252);
    }

    #[test]
    fn a_stated_delimiter_wins_over_the_one_detection_would_pick() {
        // Detection alone splits these records on the comma.
        let sample = "id,name,city;note\n1,Ann,Graz;x\n2,Bob,Linz;y\n3,Cid,Wels;z\n";
        let statement = stated(serde_json::json!({"delimiter": ";"}));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(reader.headers().unwrap(), vec!["id,name,city", "note"]);
    }

    #[test]
    fn a_stated_preamble_skips_a_title_as_wide_as_the_table() {
        // A dense title over an all-text table gives detection no evidence, so it stays in the table
        // unless the manifest states it.
        let sample = "Report;Vienna\nStation;Name\nSTEF;Stephansplatz\nAKA;Kendlerstrasse\nTAB;Taborstrasse\n";
        let statement = stated(serde_json::json!({"skipRows": 1}));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(reader.headers().unwrap(), vec!["Station", "Name"]);
    }

    #[test]
    fn a_stated_absent_header_reads_a_label_row_as_data() {
        let statement = stated(serde_json::json!({"header": false}));
        let dialect = CsvInspector::new(&statement)
            .inspect(&Content::new(b"id,name,value\n1,alpha,10\n2,beta,20\n"))
            .unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::NONE);
    }

    #[test]
    fn a_stated_header_reads_a_numeric_first_row_as_the_header() {
        let statement = stated(serde_json::json!({"header": true}));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(b"1,2,3\n4,5,6\n7,8,9\n")).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::ONE);
    }

    #[test]
    fn a_stated_header_row_count_is_kept_as_stated() {
        let sample = "Lumes;v2.10\n;NO2;O3\n;HMW;HMW\nSTEF;18,48;50,74\nTAB;NE;10,37\nKE;15,17;8,48\n";
        let statement = stated(serde_json::json!({"skipRows": 1, "headerRows": 2}));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.header_rows, HeaderRows::new(2));
        assert_eq!(reader.headers().unwrap(), vec!["", "NO2 HMW", "O3 HMW"]);
    }

    #[test]
    fn a_stated_encoding_wins_over_the_one_the_content_reads_as() {
        let statement = stated(serde_json::json!({"encoding": "latin1"}));
        let dialect = CsvInspector::new(&statement)
            .inspect(&Content::new(&ascii_rows_then("Müller".as_bytes())))
            .unwrap();

        assert_eq!(dialect.encoding.to_string(), "windows-1252");
    }

    #[test]
    fn a_whole_statement_is_the_dialect_without_detection() {
        let statement = stated(serde_json::json!({
            "delimiter": "|",
            "quote": "none",
            "skipRows": 2,
            "headerRows": 1,
            "terminator": "crlf",
            "encoding": "utf-8",
        }));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(b"")).unwrap();

        assert_eq!(dialect.delimiter, DialectByte::PIPE);
        assert_eq!(dialect.quote_char, Quote::Disabled);
        assert_eq!(dialect.preamble, Preamble::new(2));
        assert_eq!(dialect.terminator, LineTerminator::Crlf);
    }

    #[test]
    fn a_stated_value_notation_is_carried_onto_the_detected_dialect() {
        let statement = stated(serde_json::json!({"decimalSeparator": ",", "nullValues": ["NE"]}));
        let dialect = CsvInspector::new(&statement)
            .inspect(&Content::new(b"Station;NO2\nSTEF;12,45\nTAB;NE\nAKA;9,80\n"))
            .unwrap();

        assert_eq!(dialect.notation.decimal_separator, DecimalSeparator::Comma);
        assert!(dialect.notation.is_null_token("NE"));
    }

    /// Quotes escaped with a backslash, as MySQL's `SELECT ... INTO OUTFILE` writes them.
    const BACKSLASH_ESCAPED: &str = "id,name,value\n1,\"say \\\"hi\\\"\",10\n2,plain,20\n3,\"a\\\"b\",30\n";

    #[test]
    fn backslash_escaped_quotes_are_detected_as_a_backslash_escape() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(BACKSLASH_ESCAPED.as_bytes()))
            .unwrap();
        let mut reader = dialect.reader(BACKSLASH_ESCAPED.as_bytes()).unwrap();
        let names: Vec<_> = reader.records().map(|record| record.unwrap()[1].to_owned()).collect();

        assert_eq!(dialect.delimiter, DialectByte::COMMA);
        assert_eq!(dialect.quote_char, Quote::Char(DialectByte::DOUBLE_QUOTE));
        assert_eq!(dialect.escape, Escape::Char(DialectByte::BACKSLASH));
        assert_eq!(names, vec!["say \"hi\"", "plain", "a\"b"]);
    }

    #[test]
    fn doubled_quotes_are_detected_as_doubling() {
        let sample = "id,name,value\n1,\"say \"\"hi\"\"\",10\n2,plain,20\n3,\"a\"\"b\",30\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();

        assert_eq!(dialect.escape, Escape::Doubled);
    }

    #[test]
    fn a_quoted_windows_path_ending_in_a_backslash_is_detected_as_doubling_and_read_intact() {
        let sample = "path,count\n\"C:\\data\\\",1\n\"D:\\export\\\",2\n\"E:\\\",3\n";
        let dialect = CsvInspector::new(&DialectStatement::NOTHING).inspect(&Content::new(sample.as_bytes())).unwrap();
        let mut reader = dialect.reader(sample.as_bytes()).unwrap();

        assert_eq!(dialect.escape, Escape::Doubled);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["C:\\data\\", "1"]);
    }

    #[test]
    fn a_file_without_quotes_is_detected_with_no_escape() {
        let dialect = CsvInspector::new(&DialectStatement::NOTHING)
            .inspect(&Content::new(b"id,name,value\n1,alpha,10\n2,beta,20\n"))
            .unwrap();

        assert_eq!(dialect.escape, Escape::Doubled);
    }

    #[test]
    fn a_stated_escape_is_used_instead_of_detection() {
        let statement = stated(serde_json::json!({"escape": "double"}));
        let dialect = CsvInspector::new(&statement).inspect(&Content::new(BACKSLASH_ESCAPED.as_bytes())).unwrap();

        assert_eq!(dialect.escape, Escape::Doubled);
        assert_eq!(dialect.quote_char, Quote::Char(DialectByte::DOUBLE_QUOTE));
    }

    #[test]
    fn a_stated_backslash_escape_is_used_on_a_file_without_escaped_quotes() {
        let statement = stated(serde_json::json!({"escape": "\\"}));
        let dialect = CsvInspector::new(&statement)
            .inspect(&Content::new(b"id,name,value\n1,alpha,10\n2,beta,20\n"))
            .unwrap();

        assert_eq!(dialect.escape, Escape::Char(DialectByte::BACKSLASH));
        assert_ne!(dialect.quote_char, Quote::Disabled);
    }
}

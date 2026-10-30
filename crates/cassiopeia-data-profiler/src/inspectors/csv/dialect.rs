use crate::inspectors::csv::preamble::Preamble;
use ::csv::{Reader, ReaderBuilder, Terminator};
use serde::Serialize;
use std::io::{self, BufRead};

/// Line terminators supported by the CSV profiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LineTerminator {
    /// A line terminated by `\n`.
    Lf,
    /// A line terminated by `\r\n`.
    CrLf,
}

impl LineTerminator {
    /// Converts the profiler representation to the CSV parser representation.
    #[must_use]
    pub const fn as_csv(self) -> Terminator {
        match self {
            Self::Lf => Terminator::Any(b'\n'),
            Self::CrLf => Terminator::CRLF,
        }
    }

    /// Converts the profiler representation to the low-level CSV state machine representation.
    const fn as_csv_core(self) -> csv_core::Terminator {
        match self {
            Self::Lf => csv_core::Terminator::Any(b'\n'),
            Self::CrLf => csv_core::Terminator::CRLF,
        }
    }
}

/// A detected CSV dialect: the parameters needed to parse a specific CSV file.
#[derive(Debug, Clone, Serialize)]
pub struct Dialect {
    /// The field delimiter byte (comma, tab, semicolon, ...).
    pub delimiter: u8,
    /// The quote byte, or `None` when quoting is disabled.
    pub quote_char: Option<u8>,
    /// The records above the table, skipped before the header row is read.
    pub preamble: Preamble,
    /// Whether the first row after the preamble is a header row.
    pub has_headers: bool,
    /// The escape byte, if any.
    pub escape: Option<u8>,
    /// The line terminator.
    pub terminator: LineTerminator,
    /// The detected character encoding, as its canonical `encoding_rs` label (`UTF-8`,
    /// `windows-1252`, ...). The ingestor decodes the file with it so a non-UTF-8 source is read
    /// correctly rather than rejected on its first non-UTF-8 byte.
    pub encoding: String,
}

impl Dialect {
    /// Opens UTF-8 `input` as CSV under this dialect: skips the preamble, then returns a reader
    /// positioned on the header row, or on the first data row of a headerless table.
    ///
    /// Every parse of a profiled CSV goes through here, the dialect inspector's candidate scoring,
    /// the detector, and the ingestor alike, so they all agree on where the table starts and how its
    /// records split.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] when `input` cannot be read while skipping the preamble.
    pub fn reader<R: BufRead>(&self, mut input: R) -> io::Result<Reader<R>> {
        self.preamble.skip(&mut self.core_reader(), &mut input)?;
        Ok(self.reader_builder().from_reader(input))
    }

    /// The table reader configuration: non-flexible, so a record whose field count differs from the
    /// first one is a parse error. It does not skip the preamble; [`Self::reader`] does that first.
    pub(crate) fn reader_builder(&self) -> ReaderBuilder {
        let mut builder = ReaderBuilder::new();
        builder
            .delimiter(self.delimiter)
            .has_headers(self.has_headers)
            .terminator(self.terminator.as_csv())
            .escape(self.escape);
        match self.quote_char {
            Some(quote) => builder.quote(quote),
            None => builder.quoting(false),
        };
        builder
    }

    /// The state machine the preamble is skipped with, splitting records exactly as
    /// [`Self::reader_builder`]'s reader does.
    fn core_reader(&self) -> csv_core::Reader {
        let mut builder = csv_core::ReaderBuilder::new();
        builder.delimiter(self.delimiter).terminator(self.terminator.as_csv_core()).escape(self.escape);
        match self.quote_char {
            Some(quote) => builder.quote(quote),
            None => builder.quoting(false),
        };
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{
        dialect::{Dialect, LineTerminator},
        preamble::Preamble,
    };
    use pretty_assertions::assert_eq;

    fn semicolon_dialect(preamble: Preamble) -> Dialect {
        Dialect {
            delimiter: b';',
            quote_char: Some(b'"'),
            preamble,
            has_headers: true,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: "UTF-8".to_owned(),
        }
    }

    #[test]
    fn a_reader_without_preamble_reads_the_header_from_the_first_record() {
        let mut reader = semicolon_dialect(Preamble::NONE).reader(&b"a;b\n1;2\n"[..]).unwrap();

        assert_eq!(reader.headers().unwrap(), vec!["a", "b"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", "2"]);
    }

    #[test]
    fn a_reader_skips_a_preamble_record_of_another_width_before_the_header() {
        let input = b"Lumes;v2.10;25.10.26-08:00:00\nStation;NO2\nSTEF;12,45\n";
        let mut reader = semicolon_dialect(Preamble::new(1)).reader(&input[..]).unwrap();

        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "12,45"]);
    }

    #[test]
    fn a_quoted_line_break_in_the_preamble_leaves_the_header_and_first_row_intact() {
        let input = b"\"Lumes report\ngenerated 25.10.26\";v2.10\nStation;NO2\nSTEF;12,45\nAKA;9,80\n";
        let mut reader = semicolon_dialect(Preamble::new(1)).reader(&input[..]).unwrap();
        let records: Vec<_> = reader.records().map(Result::unwrap).collect();

        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2"]);
        assert_eq!(records, vec![vec!["STEF", "12,45"], vec!["AKA", "9,80"]]);
    }

    #[test]
    fn a_reader_rejects_a_record_whose_width_differs_from_the_table() {
        let mut reader = semicolon_dialect(Preamble::NONE).reader(&b"a;b\n1;2;3\n"[..]).unwrap();

        assert!(reader.records().any(|record| record.is_err()));
    }

    #[test]
    fn a_dialect_serialises_its_preamble_as_a_record_count() {
        let json = serde_json::to_value(semicolon_dialect(Preamble::new(1))).unwrap();

        assert_eq!(json["preamble"], 1);
    }
}

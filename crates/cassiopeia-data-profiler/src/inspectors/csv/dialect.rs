use crate::inspectors::csv::{header_rows::HeaderRows, preamble::Preamble};
use ::csv::{Reader, ReaderBuilder, StringRecord, Terminator};
use encoding_rs::{Encoding, UTF_8};
use serde::Serialize;
use std::io::{BufRead, Read};

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
    /// The records after the preamble that form the header, joined per column into field names.
    pub header_rows: HeaderRows,
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
    /// The character encoding the source is decoded from, resolved from its label. A label
    /// `encoding_rs` does not know falls back to UTF-8, the encoding every parse below expects.
    #[must_use]
    pub fn character_encoding(&self) -> &'static Encoding {
        Encoding::for_label(self.encoding.as_bytes()).unwrap_or(UTF_8)
    }

    /// Opens `input`, already decoded to UTF-8 with [`Self::character_encoding`], as CSV under this dialect: skips the preamble, joins a stacked header into
    /// one row of field names, and returns a reader whose next record is the first data row.
    ///
    /// Every parse of a profiled CSV goes through here, the dialect inspector's candidate scoring,
    /// the detector, and the ingestor alike, so they all agree on where the table starts, what its
    /// field names are, and how its records split.
    ///
    /// # Errors
    ///
    /// Returns a [`csv::Error`] when `input` cannot be read while skipping the preamble or when a
    /// header record cannot be parsed.
    pub fn reader<R: BufRead>(&self, mut input: R) -> csv::Result<Reader<R>> {
        self.preamble.skip(&mut self.core_reader(), &mut input)?;
        let mut reader = self.reader_builder().from_reader(input);
        if self.header_rows.records() > 1 {
            let names = self.joined_header(&mut reader)?;
            reader.set_headers(names);
        }
        Ok(reader)
    }

    /// Reads the header records and joins each column's non-blank cells, top to bottom, with a
    /// space: a name row `NO2`, an aggregation row `HMW`, and a unit row `µg/m³` give the field name
    /// `NO2 HMW µg/m³`. A column whose header cells are all blank gets an empty name.
    fn joined_header<R: Read>(&self, reader: &mut Reader<R>) -> csv::Result<StringRecord> {
        let mut names: Vec<String> = reader.headers()?.iter().map(|cell| cell.trim().to_owned()).collect();
        let mut row = StringRecord::new();
        for _ in 1..self.header_rows.records() {
            if !reader.read_record(&mut row)? {
                break;
            }
            for (name, cell) in names.iter_mut().zip(row.iter()) {
                let cell = cell.trim();
                if cell.is_empty() {
                    continue;
                }
                if !name.is_empty() {
                    name.push(' ');
                }
                name.push_str(cell);
            }
        }
        Ok(StringRecord::from(names))
    }

    /// The table reader configuration: non-flexible, so a record whose field count differs from the
    /// first one is a parse error. It does not skip the preamble; [`Self::reader`] does that first.
    pub(crate) fn reader_builder(&self) -> ReaderBuilder {
        let mut builder = ReaderBuilder::new();
        builder
            .delimiter(self.delimiter)
            .has_headers(self.header_rows.is_present())
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
        header_rows::HeaderRows,
        preamble::Preamble,
    };
    use pretty_assertions::assert_eq;

    fn semicolon_dialect(preamble: Preamble) -> Dialect {
        Dialect {
            delimiter: b';',
            quote_char: Some(b'"'),
            preamble,
            header_rows: HeaderRows::ONE,
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

    #[test]
    fn a_stacked_header_is_joined_per_column_with_spaces() {
        let input = "Lumes;v2.10\n;Zeit-NO2;NO2;PM10;PM10\n;;HMW;MW24;HMW\n;MESZ;µg/m³;µg/m³;µg/m³\nSTEF;30.10.2026, 10:00;18,48;12,48;12,27\n";
        let dialect = Dialect {
            header_rows: HeaderRows::new(3),
            ..semicolon_dialect(Preamble::new(1))
        };
        let mut reader = dialect.reader(input.as_bytes()).unwrap();

        assert_eq!(
            reader.headers().unwrap(),
            vec!["", "Zeit-NO2 MESZ", "NO2 HMW µg/m³", "PM10 MW24 µg/m³", "PM10 HMW µg/m³"]
        );
        assert_eq!(
            reader.records().next().unwrap().unwrap(),
            vec!["STEF", "30.10.2026, 10:00", "18,48", "12,48", "12,27"]
        );
    }

    #[test]
    fn a_headerless_dialect_reads_the_first_record_as_data() {
        let dialect = Dialect {
            header_rows: HeaderRows::NONE,
            ..semicolon_dialect(Preamble::NONE)
        };
        let mut reader = dialect.reader(&b"1;2\n3;4\n"[..]).unwrap();

        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", "2"]);
    }
}

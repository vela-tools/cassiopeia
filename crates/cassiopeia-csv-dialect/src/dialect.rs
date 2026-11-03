use crate::{
    character_encoding::CharacterEncoding,
    dialect_byte::DialectByte,
    escape::Escape,
    header_rows::HeaderRows,
    line_terminator::LineTerminator,
    preamble::Preamble,
    quote::Quote,
    record_end::{RecordEnd, skip_record},
    table_reader::TableReader,
    value_notation::ValueNotation,
};
use csv::{Reader, ReaderBuilder, StringRecord};
use csv_core::{Reader as CoreReader, ReaderBuilder as CoreReaderBuilder};
use encoding_rs::Encoding;
use serde::Serialize;
use std::io::{BufRead, Read};

/// A CSV dialect: the parameters needed to parse a specific CSV file, detected, stated in a
/// manifest, or both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dialect {
    /// The field delimiter (comma, tab, semicolon, ...).
    pub delimiter: DialectByte,
    /// The quote character, or none when quoting is disabled.
    pub quote_char: Quote,
    /// The records above the table, skipped before the header row is read.
    pub preamble: Preamble,
    /// The records after the preamble that form the header, joined per column into field names.
    pub header_rows: HeaderRows,
    /// How a quote inside a quoted field is escaped. It has no effect when quoting is disabled.
    pub escape: Escape,
    /// The line terminator.
    pub terminator: LineTerminator,
    /// The character encoding. The ingestor decodes the file with it so a non-UTF-8 source is read
    /// correctly rather than rejected on its first non-UTF-8 byte.
    pub encoding: CharacterEncoding,
    /// How the values in the fields are written: their decimal separator and null tokens.
    pub notation: ValueNotation,
}

impl Dialect {
    /// The character encoding the source is decoded from.
    #[must_use]
    pub const fn character_encoding(&self) -> &'static Encoding {
        self.encoding.encoding()
    }

    /// Whether the delimiter, the quote, and the escape byte are distinct characters, as each must be
    /// to play its own part: a delimiter that is also the quote or the escape would split a field
    /// where it was meant to quote or escape one. Doubling has no byte of its own, and disabled
    /// quoting none either, so neither can collide.
    #[must_use]
    pub fn has_distinct_bytes(&self) -> bool {
        let delimiter = Some(self.delimiter.byte());
        let quote = self.quote_char.byte();
        let escape = self.escape.byte();
        (quote.is_none() || quote != delimiter) && (escape.is_none() || (escape != delimiter && escape != quote))
    }

    /// Opens `input`, already decoded to UTF-8 with [`Self::character_encoding`], as CSV under this
    /// dialect: skips the preamble, joins a stacked header into one row of field names, and returns a
    /// reader whose next row is the first data row, each row brought to the table's width or
    /// reported as misshapen.
    ///
    /// Every parse of a profiled CSV goes through here, the dialect inspector's candidate scoring,
    /// the detector, and the ingestor alike, so they all agree on where the table starts, what its
    /// field names are, and how its records split.
    ///
    /// # Errors
    ///
    /// Returns a [`csv::Error`] when `input` cannot be read while skipping the preamble or when a
    /// header record cannot be parsed.
    pub fn reader<R: BufRead>(&self, mut input: R) -> csv::Result<TableReader<R>> {
        self.preamble.skip(&mut self.core_reader(), &mut input)?;
        let mut reader = self.reader_builder().from_reader(input);
        if self.header_rows.records() > 1 {
            let names = self.joined_header(&mut reader)?;
            reader.set_headers(names);
        }
        TableReader::new(reader, self.preamble)
    }

    /// Reads the header records and joins each column's non-blank cells, top to bottom, with a
    /// space: a name row `NO2`, an aggregation row `HMW`, and a unit row `µg/m³` give the field name
    /// `NO2 HMW µg/m³`. A column whose header cells are all blank gets an empty name.
    ///
    /// The first header record sets the table's width. A later header record of another width is
    /// still joined by position, over the columns it reaches, rather than failing the file: a unit
    /// row a spreadsheet wrote one cell short names the columns it has.
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

    /// The leading part of `sample` that holds only whole records under this dialect: everything up
    /// to the terminator of its last complete record. A record the end of `sample` cuts off, such as
    /// one cut inside a quoted line break, is left out, as is a last record without a terminator.
    ///
    /// Where a sample of a file is cut depends on the dialect, since a quote decides which line
    /// breaks end a record; cutting it here keeps a candidate dialect from being scored on a
    /// truncated last record.
    #[must_use]
    pub fn complete_records<'a>(&self, sample: &'a [u8]) -> &'a [u8] {
        let mut core = self.core_reader();
        let mut rest = sample;
        let mut complete = 0;
        // Reading a byte slice cannot fail, so the scan ends only at the first record the end of
        // `sample` cuts off.
        while let Ok(RecordEnd::Terminated) = skip_record(&mut core, &mut rest) {
            complete = sample.len() - rest.len();
        }
        &sample[..complete]
    }

    /// The table reader configuration: flexible, so a record of any field count is read, and
    /// [`TableReader`] decides whether it fits the table. It does not skip the preamble;
    /// [`Self::reader`] does that first.
    ///
    /// Both readers leave `double_quote` on whatever the escape, so a doubled quote reads as one
    /// under an escape byte too; `csv_core` checks for the escape byte only after the quote.
    #[must_use]
    pub fn reader_builder(&self) -> ReaderBuilder {
        let mut builder = ReaderBuilder::new();
        builder
            .delimiter(self.delimiter.byte())
            .has_headers(self.header_rows.is_present())
            .flexible(true)
            .terminator(self.terminator.as_csv())
            .escape(self.escape.byte());
        match self.quote_char.byte() {
            Some(quote) => builder.quote(quote),
            None => builder.quoting(false),
        };
        builder
    }

    /// The state machine the preamble is skipped with, splitting records exactly as
    /// [`Self::reader_builder`]'s reader does.
    fn core_reader(&self) -> CoreReader {
        let mut builder = CoreReaderBuilder::new();
        builder
            .delimiter(self.delimiter.byte())
            .terminator(self.terminator.as_csv_core())
            .escape(self.escape.byte());
        match self.quote_char.byte() {
            Some(quote) => builder.quote(quote),
            None => builder.quoting(false),
        };
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        character_encoding::CharacterEncoding,
        dialect::Dialect,
        dialect_byte::DialectByte,
        escape::Escape,
        header_rows::HeaderRows,
        line_terminator::LineTerminator,
        preamble::Preamble,
        quote::Quote,
        value_notation::ValueNotation,
    };
    use encoding_rs::UTF_8;
    use pretty_assertions::assert_eq;

    fn semicolon_dialect(preamble: Preamble) -> Dialect {
        Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble,
            header_rows: HeaderRows::ONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
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
    fn a_stacked_header_row_one_cell_short_is_joined_over_the_columns_it_reaches() {
        let input = "Station;NO2;O3\n;HMW;HMW\n;µg/m³\nSTEF;18,48;50,74\nTAB;NE;10,37\n";
        let dialect = Dialect {
            header_rows: HeaderRows::new(3),
            ..semicolon_dialect(Preamble::NONE)
        };
        let mut reader = dialect.reader(input.as_bytes()).unwrap();
        let records: Vec<_> = reader.records().map(Result::unwrap).collect();

        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2 HMW µg/m³", "O3 HMW"]);
        assert_eq!(records, vec![vec!["STEF", "18,48", "50,74"], vec!["TAB", "NE", "10,37"]]);
    }

    #[test]
    fn a_stacked_header_row_one_cell_long_is_joined_and_its_extra_cell_dropped() {
        let input = "Station;NO2\n;HMW;extra\nSTEF;18,48\n";
        let dialect = Dialect {
            header_rows: HeaderRows::new(2),
            ..semicolon_dialect(Preamble::NONE)
        };
        let mut reader = dialect.reader(input.as_bytes()).unwrap();

        assert_eq!(reader.headers().unwrap(), vec!["Station", "NO2 HMW"]);
        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["STEF", "18,48"]);
    }

    #[test]
    fn a_dialect_whose_delimiter_is_its_quote_or_escape_has_no_distinct_bytes() {
        let quote_delimiter = Dialect {
            delimiter: DialectByte::DOUBLE_QUOTE,
            ..semicolon_dialect(Preamble::NONE)
        };
        let escape_delimiter = Dialect {
            delimiter: DialectByte::BACKSLASH,
            ..backslash_dialect(Preamble::NONE)
        };

        assert!(!quote_delimiter.has_distinct_bytes());
        assert!(!escape_delimiter.has_distinct_bytes());
    }

    #[test]
    fn a_dialect_of_a_delimiter_a_quote_and_doubling_has_distinct_bytes() {
        assert!(semicolon_dialect(Preamble::NONE).has_distinct_bytes());
        assert!(backslash_dialect(Preamble::NONE).has_distinct_bytes());
        assert!(
            Dialect {
                quote_char: Quote::Disabled,
                ..semicolon_dialect(Preamble::NONE)
            }
            .has_distinct_bytes()
        );
    }

    #[test]
    fn a_sample_cut_inside_a_quoted_line_break_keeps_only_its_complete_records() {
        let sample = b"a;b\n1;\"x\ny\"\n2;\"x\n";

        assert_eq!(semicolon_dialect(Preamble::NONE).complete_records(sample), b"a;b\n1;\"x\ny\"\n");
    }

    #[test]
    fn a_sample_ending_on_a_terminator_is_complete() {
        let sample = b"a;b\n1;\"x\ny\"\n";

        assert_eq!(semicolon_dialect(Preamble::NONE).complete_records(sample), sample);
    }

    #[test]
    fn a_last_record_without_a_terminator_is_not_complete() {
        assert_eq!(semicolon_dialect(Preamble::NONE).complete_records(b"a;b\n1;2"), b"a;b\n");
    }

    #[test]
    fn without_quoting_every_line_break_ends_a_record() {
        let dialect = Dialect {
            quote_char: Quote::Disabled,
            ..semicolon_dialect(Preamble::NONE)
        };
        let sample = b"a;b\n1;\"x\n";

        assert_eq!(dialect.complete_records(sample), sample);
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

    fn backslash_dialect(preamble: Preamble) -> Dialect {
        Dialect {
            escape: Escape::Char(DialectByte::BACKSLASH),
            ..semicolon_dialect(preamble)
        }
    }

    #[test]
    fn a_backslash_escaped_dialect_reads_an_escaped_quote_as_a_quote() {
        let mut reader = backslash_dialect(Preamble::NONE)
            .reader(
                &br#"id;name
1;"say \"hi\""
"#[..],
            )
            .unwrap();

        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", r#"say "hi""#]);
    }

    #[test]
    fn a_backslash_escaped_dialect_reads_an_escaped_backslash_as_one_backslash() {
        let mut reader = backslash_dialect(Preamble::NONE)
            .reader(
                &br#"id;path
1;"C:\\data\\"
"#[..],
            )
            .unwrap();

        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", r"C:\data\"]);
    }

    #[test]
    fn the_doubling_dialect_reads_a_doubled_quote_as_a_quote() {
        let mut reader = semicolon_dialect(Preamble::NONE)
            .reader(
                &br#"id;name
1;"say ""hi"""
"#[..],
            )
            .unwrap();

        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", r#"say "hi""#]);
    }

    #[test]
    fn a_backslash_escaped_dialect_still_reads_a_doubled_quote_as_a_quote() {
        let mut reader = backslash_dialect(Preamble::NONE)
            .reader(
                &br#"id;name
1;"say ""hi"""
"#[..],
            )
            .unwrap();

        assert_eq!(reader.records().next().unwrap().unwrap(), vec!["1", r#"say "hi""#]);
    }

    #[test]
    fn both_readers_split_a_backslash_escaped_field_with_a_line_break_alike() {
        // The preamble is skipped by the `csv_core` reader and the table read by the `csv` reader,
        // so an escaped quote in the preamble's quoted line break must end its record at the same
        // place in both, or the header is read from the middle of the title.
        let input = b"\"Report \\\"live\\\"\ngenerated 25.10.26\";v2\nid;name\n1;\"say \\\"hi\\\"\nagain\"\n2;plain\n";
        let mut reader = backslash_dialect(Preamble::new(1)).reader(&input[..]).unwrap();
        let records: Vec<_> = reader.records().map(Result::unwrap).collect();

        assert_eq!(reader.headers().unwrap(), vec!["id", "name"]);
        assert_eq!(records, vec![vec!["1", "say \"hi\"\nagain"], vec!["2", "plain"]]);
    }

    #[test]
    fn a_dialect_serialises_its_escape_as_a_manifest_string() {
        let json = serde_json::to_value(backslash_dialect(Preamble::NONE)).unwrap();

        assert_eq!(json["escape"], "\\");
    }
}

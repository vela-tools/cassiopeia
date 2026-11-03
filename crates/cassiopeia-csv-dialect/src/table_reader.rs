use crate::{misshapen_row::MisshapenRow, preamble::Preamble};
use csv::{Reader, ReaderBuilder, StringRecord};
use std::{cmp::Ordering, io::Read, iter};
use thiserror::Error;

/// Why a record read from a table is no row of it.
#[derive(Debug, Error)]
pub enum TableRowError {
    /// The record's fields do not fit the table's, so it is skipped and reading goes on past it.
    #[error("a record of another width than the table's")]
    Misshapen(MisshapenRow),
    /// The record could not be read at all.
    #[error(transparent)]
    Csv(#[from] csv::Error),
}

/// Whether a record fits its table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// The record has the table's width, or has been brought to it without losing a value.
    Fits,
    /// The record has another width, and which of its fields is missing or extra cannot be told.
    Misshapen,
}

/// How many fields every row of a table has, and how many of them a record may leave out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TableWidth {
    /// The table's field count: its header's, or its first record's when it has none.
    fields: usize,
    /// The fewest fields a row may have: the fields of the record that set the width up to its last
    /// non-blank one. The blank fields after it, such as the empty field a trailing delimiter
    /// leaves, are the ones a row that stops short of them leaves out, so it loses no value.
    least: usize,
}

impl TableWidth {
    /// The width `record` sets, the header or, in a table without one, the first record: its
    /// fields, of which a row may leave out the blank last ones.
    ///
    /// The width-setting record is the header candidate the inspector weighs, read as the table's
    /// first row while the table's header is undecided, so a header and a first row follow one rule.
    fn of(record: &StringRecord) -> TableWidth {
        let blank = record.iter().rev().take_while(|field| is_blank(field)).count();
        TableWidth {
            fields: record.len(),
            least: record.len() - blank,
        }
    }

    /// Whether `record` fits this width, bringing it to the width when it fits without losing a
    /// value: blank fields past the width are cut, as a trailing delimiter leaves them, and the blank
    /// last fields a row stops short of are filled with empty fields.
    fn fit(self, record: &mut StringRecord) -> Fit {
        match record.len().cmp(&self.fields) {
            Ordering::Equal => Fit::Fits,
            Ordering::Greater if record.iter().skip(self.fields).all(is_blank) => {
                record.truncate(self.fields);
                Fit::Fits
            }
            Ordering::Less if record.len() >= self.least => {
                (record.len()..self.fields).for_each(|_| record.push_field(""));
                Fit::Fits
            }
            Ordering::Greater | Ordering::Less => Fit::Misshapen,
        }
    }
}

/// Whether `field` holds nothing but whitespace.
fn is_blank(field: &str) -> bool {
    field.trim().is_empty()
}

/// A reader of a CSV table's rows, each brought to the table's width or reported as misshapen.
///
/// RFC 4180 clause 2.4 has every record of a file hold the same number of fields, and the `csv`
/// crate's strict reader fails every record that does not. Files from spreadsheets and databases
/// often end every line, or the header line alone, with a delimiter, which gives the record one
/// more field, empty, than the others have. That field holds no value, so a record whose only extra
/// or missing fields are such empty trailing fields is read as a row of the table. Any other record
/// of another width is a [`MisshapenRow`].
pub struct TableReader<R> {
    /// The reader, flexible, so a record of another width reaches the width check here.
    reader: Reader<R>,
    /// The records above the table, which count toward the row a misshapen record sits on.
    preamble: Preamble,
    /// The table's width, set by its header, or by its first record once that is read.
    width: Option<TableWidth>,
}

impl<R: Read> TableReader<R> {
    /// Reads the table `reader` is positioned at, below `preamble`, reading its header first when
    /// it has one.
    ///
    /// # Errors
    ///
    /// Returns a [`csv::Error`] when the header record cannot be read.
    pub(crate) fn new(mut reader: Reader<R>, preamble: Preamble) -> csv::Result<TableReader<R>> {
        let width = if reader.has_headers() {
            Some(TableWidth::of(reader.headers()?))
        } else {
            None
        };
        Ok(TableReader { reader, preamble, width })
    }

    /// Reads `input` as a table under the `csv` crate's defaults: comma-delimited, quoted with
    /// `"`, with one header record and no preamble. It reads a file no dialect was detected for.
    ///
    /// # Errors
    ///
    /// Returns a [`csv::Error`] when the header record cannot be read.
    pub fn with_csv_defaults(input: R) -> csv::Result<TableReader<R>> {
        TableReader::new(ReaderBuilder::new().flexible(true).from_reader(input), Preamble::NONE)
    }

    /// The table's field names, or its first record when it has no header.
    ///
    /// # Errors
    ///
    /// Returns a [`csv::Error`] when the first record cannot be read.
    pub fn headers(&mut self) -> csv::Result<&StringRecord> {
        self.reader.headers()
    }

    /// The table's rows, each with the table's width, in file order.
    pub fn records(&mut self) -> impl Iterator<Item = Result<StringRecord, TableRowError>> + '_ {
        iter::from_fn(move || self.next_row())
    }

    /// The next row, or `None` at the end of the input.
    fn next_row(&mut self) -> Option<Result<StringRecord, TableRowError>> {
        // Every record the reader has read so far, header records included, sits above this one.
        let above = self.reader.position().record();
        let mut record = StringRecord::new();
        match self.reader.read_record(&mut record) {
            Ok(true) => Some(self.fitted(above, record)),
            Ok(false) => None,
            Err(error) => Some(Err(TableRowError::Csv(error))),
        }
    }

    /// `record`, the one `above` records into the table, brought to the table's width, or the
    /// misshapen row it is.
    fn fitted(&mut self, above: u64, mut record: StringRecord) -> Result<StringRecord, TableRowError> {
        let width = *self.width.get_or_insert_with(|| TableWidth::of(&record));
        match width.fit(&mut record) {
            Fit::Fits => Ok(record),
            Fit::Misshapen => Err(TableRowError::Misshapen(MisshapenRow::below(self.preamble, above, record.len(), width.fields))),
        }
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
        misshapen_row::MisshapenRow,
        preamble::Preamble,
        quote::Quote,
        row_number::RowNumber,
        table_reader::{TableReader, TableRowError},
        value_notation::ValueNotation,
    };
    use csv::StringRecord;
    use encoding_rs::UTF_8;
    use pretty_assertions::assert_eq;

    fn semicolon_dialect(preamble: Preamble, header_rows: HeaderRows) -> Dialect {
        Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble,
            header_rows,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }
    }

    fn rows(dialect: &Dialect, input: &[u8]) -> Vec<Result<StringRecord, MisshapenRow>> {
        dialect
            .reader(input)
            .unwrap()
            .records()
            .map(|row| {
                row.map_err(|error| match error {
                    TableRowError::Misshapen(row) => row,
                    TableRowError::Csv(error) => panic!("{error}"),
                })
            })
            .collect()
    }

    fn headed(input: &[u8]) -> Vec<Result<StringRecord, MisshapenRow>> {
        rows(&semicolon_dialect(Preamble::NONE, HeaderRows::ONE), input)
    }

    fn headerless(input: &[u8]) -> Vec<Result<StringRecord, MisshapenRow>> {
        rows(&semicolon_dialect(Preamble::NONE, HeaderRows::NONE), input)
    }

    fn row(fields: &[&str]) -> StringRecord {
        StringRecord::from(fields.to_vec())
    }

    fn misshapen(above: u64, fields: usize, table_fields: usize) -> Result<StringRecord, MisshapenRow> {
        Err(MisshapenRow {
            row: RowNumber::after(above),
            fields,
            table_fields,
        })
    }

    #[test]
    fn a_header_ending_in_a_delimiter_reads_the_rows_without_one_under_an_unnamed_last_column() {
        let mut reader = semicolon_dialect(Preamble::NONE, HeaderRows::ONE).reader(&b"a;b;c;\n1;2;3\n"[..]).unwrap();

        assert_eq!(reader.headers().unwrap(), vec!["a", "b", "c", ""]);
        assert_eq!(reader.records().map(Result::unwrap).collect::<Vec<_>>(), vec![vec!["1", "2", "3", ""]]);
    }

    #[test]
    fn rows_ending_in_a_delimiter_under_a_header_without_one_lose_only_the_empty_field() {
        assert_eq!(headed(b"a;b;c\n1;2;3;\n4;5;6\n"), vec![Ok(row(&["1", "2", "3"])), Ok(row(&["4", "5", "6"]))]);
    }

    #[test]
    fn several_blank_fields_past_the_table_are_cut_whitespace_included() {
        assert_eq!(headed(b"a;b\n1;2;; \n"), vec![Ok(row(&["1", "2"]))]);
    }

    #[test]
    fn a_row_that_stops_short_of_a_named_column_is_misshapen() {
        assert_eq!(headed(b"a;b;c;\n1;2\n"), vec![misshapen(1, 2, 4)]);
    }

    #[test]
    fn an_extra_field_that_holds_a_value_is_misshapen() {
        assert_eq!(headed(b"a;b\n1;2;x\n"), vec![misshapen(1, 3, 2)]);
    }

    #[test]
    fn a_headerless_table_whose_first_row_ends_in_a_delimiter_reads_the_rows_without_one() {
        assert_eq!(headerless(b"1;2;\n3;4\n"), vec![Ok(row(&["1", "2", ""])), Ok(row(&["3", "4", ""]))]);
    }

    #[test]
    fn a_headerless_row_short_of_a_value_in_the_first_row_is_misshapen() {
        assert_eq!(headerless(b"1;2;x\n3;4\n"), vec![Ok(row(&["1", "2", "x"])), misshapen(1, 2, 3)]);
    }

    #[test]
    fn a_headerless_table_cuts_a_trailing_delimiter_s_empty_field() {
        assert_eq!(headerless(b"1;2\n3;4;\n"), vec![Ok(row(&["1", "2"])), Ok(row(&["3", "4"]))]);
    }

    #[test]
    fn a_short_row_is_placed_on_its_row_of_the_file_below_the_header() {
        assert_eq!(headed(b"id;name;value\n1;a;10\n2;b\n3;c;30\n")[1], misshapen(2, 2, 3));
    }

    #[test]
    fn a_preamble_above_the_table_counts_toward_the_row() {
        let rows = rows(
            &semicolon_dialect(Preamble::new(2), HeaderRows::ONE),
            b"Lumes\nv2.10\nid;name;value\n1;a;10;extra\n",
        );

        assert_eq!(rows, vec![misshapen(3, 4, 3)]);
    }

    #[test]
    fn a_quoted_line_break_above_counts_its_record_once() {
        assert_eq!(headed(b"id;note\n1;\"two\nlines\"\nSource: office\n")[1], misshapen(2, 1, 2));
    }

    #[test]
    fn a_crlf_file_places_a_row_as_an_lf_file_does() {
        let dialect = Dialect {
            terminator: LineTerminator::Crlf,
            ..semicolon_dialect(Preamble::NONE, HeaderRows::ONE)
        };

        assert_eq!(rows(&dialect, b"id;name\r\n1;a\r\n2\r\n")[1], misshapen(2, 1, 2));
    }

    #[test]
    fn a_stacked_header_counts_every_header_record_toward_the_row() {
        let rows = rows(&semicolon_dialect(Preamble::NONE, HeaderRows::new(2)), b";NO2\n;HMW\nSTEF;12\nTAB\n");

        assert_eq!(rows, vec![Ok(row(&["STEF", "12"])), misshapen(3, 1, 2)]);
    }

    #[test]
    fn the_csv_defaults_read_a_comma_table_under_its_header() {
        let mut reader = TableReader::with_csv_defaults(&b"id,name,\n1,Ada\n"[..]).unwrap();

        assert_eq!(reader.headers().unwrap(), vec!["id", "name", ""]);
        assert_eq!(reader.records().map(Result::unwrap).collect::<Vec<_>>(), vec![vec!["1", "Ada", ""]]);
    }
}

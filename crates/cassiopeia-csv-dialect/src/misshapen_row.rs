use crate::{preamble::Preamble, row_number::RowNumber};
use csv::{Error, ErrorKind};

/// A CSV record whose number of fields differs from the table's, such as a row one field short, a
/// row with an extra field, or a note below the table.
///
/// RFC 4180 clause 2.4 has every record of a file hold the same number of fields, so a record that
/// does not is no row of the table. Which of its fields is missing or extra cannot be told, so it is
/// skipped whole rather than padded or cut to fit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MisshapenRow {
    /// Where the record sits in the file.
    pub row: RowNumber,
    /// How many fields the record has.
    pub fields: u64,
    /// How many fields every record of the table has: as many as its header, or as its first record
    /// when it has none.
    pub table_fields: u64,
}

impl MisshapenRow {
    /// The misshapen record `error` reports, read below `preamble` by a reader that is not flexible,
    /// or `None` when `error` is any other failure.
    ///
    /// The reader has consumed the record when it reports it, so reading can go on past it.
    #[must_use]
    pub fn of(error: &Error, preamble: Preamble) -> Option<MisshapenRow> {
        let ErrorKind::UnequalLengths {
            pos: Some(position),
            expected_len,
            len,
        } = error.kind()
        else {
            return None;
        };
        // The reader starts counting records below the preamble, from 0 and header included.
        let above = preamble.records().saturating_add(position.record());
        Some(MisshapenRow {
            row: RowNumber::after(above),
            fields: *len,
            table_fields: *expected_len,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{misshapen_row::MisshapenRow, preamble::Preamble, row_number::RowNumber};
    use csv::{Error, ReaderBuilder};
    use pretty_assertions::assert_eq;
    use std::io;

    fn errors(input: &[u8]) -> Vec<Error> {
        ReaderBuilder::new()
            .delimiter(b';')
            .from_reader(input)
            .records()
            .filter_map(Result::err)
            .collect()
    }

    #[test]
    fn a_short_row_is_placed_on_its_row_of_the_file_below_the_header() {
        let errors = errors(b"id;name;value\n1;a;10\n2;b\n3;c;30\n");

        assert_eq!(
            MisshapenRow::of(&errors[0], Preamble::NONE),
            Some(MisshapenRow {
                row: RowNumber::after(2),
                fields: 2,
                table_fields: 3,
            })
        );
    }

    #[test]
    fn a_preamble_above_the_table_counts_toward_the_row() {
        let errors = errors(b"id;name;value\n1;a;10;extra\n");

        assert_eq!(MisshapenRow::of(&errors[0], Preamble::new(2)).map(|row| row.row), Some(RowNumber::after(3)));
    }

    #[test]
    fn a_quoted_line_break_above_counts_its_record_once() {
        let errors = errors(b"id;note\n1;\"two\nlines\"\nSource: office\n");

        assert_eq!(MisshapenRow::of(&errors[0], Preamble::NONE).map(|row| row.row), Some(RowNumber::after(2)));
    }

    #[test]
    fn a_crlf_file_places_a_row_as_an_lf_file_does() {
        let errors = errors(b"id;name\r\n1;a\r\n2\r\n");

        assert_eq!(MisshapenRow::of(&errors[0], Preamble::NONE).map(|row| row.row), Some(RowNumber::after(2)));
    }

    #[test]
    fn any_other_read_failure_is_no_misshapen_row() {
        assert_eq!(MisshapenRow::of(&Error::from(io::Error::other("disk gone")), Preamble::NONE), None);
    }
}

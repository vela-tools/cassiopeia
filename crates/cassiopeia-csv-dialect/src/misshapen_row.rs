use crate::{preamble::Preamble, row_number::RowNumber};

/// A CSV record whose number of fields differs from the table's, such as a row one field short, a
/// row with an extra field, or a note below the table.
///
/// RFC 4180 clause 2.4 has every record of a file hold the same number of fields, so a record that
/// does not is no row of the table. Which of its fields is missing or extra cannot be told, so it is
/// skipped whole rather than padded or cut to fit. A record whose only extra or missing fields are
/// the empty ones a trailing delimiter leaves is no misshapen row; see
/// [`TableReader`](crate::table_reader::TableReader).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MisshapenRow {
    /// Where the record sits in the file.
    pub row: RowNumber,
    /// How many fields the record has.
    pub fields: usize,
    /// How many fields every record of the table has: as many as its header, or as its first record
    /// when it has none.
    pub table_fields: usize,
}

impl MisshapenRow {
    /// The misshapen record of `fields` fields in a table of `table_fields`, read `above` records
    /// into the table, header records included, below `preamble`.
    #[must_use]
    pub(crate) const fn below(preamble: Preamble, above: u64, fields: usize, table_fields: usize) -> MisshapenRow {
        MisshapenRow {
            row: RowNumber::after(preamble.records().saturating_add(above)),
            fields,
            table_fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{misshapen_row::MisshapenRow, preamble::Preamble, row_number::RowNumber};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_record_below_the_header_is_numbered_from_the_top_of_the_file() {
        assert_eq!(
            MisshapenRow::below(Preamble::NONE, 2, 2, 3),
            MisshapenRow {
                row: RowNumber::after(2),
                fields: 2,
                table_fields: 3,
            }
        );
    }

    #[test]
    fn the_preamble_counts_toward_the_row() {
        assert_eq!(MisshapenRow::below(Preamble::new(2), 1, 4, 3).row, RowNumber::after(3));
    }
}

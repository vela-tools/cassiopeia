use cassiopeia_csv_dialect::misshapen_row::MisshapenRow;

/// How many misshapen rows are kept to name, however many a file has, so a file whose every other
/// row is ragged costs no more memory than one with a single such row.
pub const NAMED_ROWS: usize = 3;

/// The rows of one CSV file skipped because their field count differs from the table's: the first
/// [`NAMED_ROWS`] of them, how many there were in all, and how many of those went unnamed.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct MisshapenRows {
    /// The first rows skipped, in file order.
    named: Vec<MisshapenRow>,
    /// How many rows were skipped, the named ones included.
    total: u64,
    /// How many rows were skipped past the named ones.
    unnamed: u64,
}

impl MisshapenRows {
    /// Counts one more skipped row, keeping it to name while fewer than [`NAMED_ROWS`] are kept.
    pub fn record(&mut self, row: MisshapenRow) {
        if self.named.len() < NAMED_ROWS {
            self.named.push(row);
        } else {
            self.unnamed = self.unnamed.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
    }

    /// The first rows skipped, in file order.
    #[must_use]
    pub fn named(&self) -> &[MisshapenRow] {
        &self.named
    }

    /// How many rows were skipped in all.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// How many rows were skipped past the named ones, which a report counts rather than names.
    #[must_use]
    pub const fn unnamed(&self) -> u64 {
        self.unnamed
    }
}

#[cfg(test)]
mod tests {
    use crate::csv::misshapen_rows::{MisshapenRows, NAMED_ROWS};
    use cassiopeia_csv_dialect::{misshapen_row::MisshapenRow, row_number::RowNumber};
    use pretty_assertions::assert_eq;

    fn row(above: u64) -> MisshapenRow {
        MisshapenRow {
            row: RowNumber::after(above),
            fields: 1,
            table_fields: 3,
        }
    }

    #[test]
    fn nothing_skipped_names_and_counts_nothing() {
        let rows = MisshapenRows::default();

        assert_eq!(rows.named(), []);
        assert_eq!(rows.total(), 0);
        assert_eq!(rows.unnamed(), 0);
    }

    #[test]
    fn only_the_first_rows_are_named_but_every_row_is_counted() {
        let mut rows = MisshapenRows::default();
        (0..5).for_each(|above| rows.record(row(above)));

        assert_eq!(rows.named().len(), NAMED_ROWS);
        assert_eq!(rows.named()[0], row(0));
        assert_eq!(rows.named()[2], row(2));
        assert_eq!(rows.total(), 5);
        assert_eq!(rows.unnamed(), 2);
    }

    #[test]
    fn rows_that_all_fit_the_named_ones_leave_none_unnamed() {
        let mut rows = MisshapenRows::default();
        (0..3).for_each(|above| rows.record(row(above)));

        assert_eq!(rows.total(), 3);
        assert_eq!(rows.unnamed(), 0);
    }
}

use derive_more::Display;
use std::num::NonZeroU64;

/// The one-based position of a record in a CSV file, counted from the top with the preamble and
/// header records included, as a spreadsheet numbers the rows of a file without blank lines.
///
/// A record is counted once however many lines a quoted line break makes it span, and a blank line
/// is no record, so this is not a line number.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RowNumber(NonZeroU64);

impl RowNumber {
    /// The row `records` records below the first.
    #[must_use]
    pub const fn after(records: u64) -> RowNumber {
        RowNumber(NonZeroU64::MIN.saturating_add(records))
    }
}

#[cfg(test)]
mod tests {
    use crate::row_number::RowNumber;
    use pretty_assertions::assert_eq;

    #[test]
    fn no_records_above_is_row_one() {
        assert_eq!(RowNumber::after(0).to_string(), "1");
        assert_eq!(RowNumber::after(5).to_string(), "6");
    }
}

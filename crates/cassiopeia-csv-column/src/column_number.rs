use derive_more::Display;
use std::num::NonZeroUsize;

/// The one-based position of a column in a CSV table, as a spreadsheet counts columns.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ColumnNumber(NonZeroUsize);

impl ColumnNumber {
    /// The number of the column at the zero-based `index`.
    #[must_use]
    pub const fn of_index(index: usize) -> ColumnNumber {
        ColumnNumber(NonZeroUsize::MIN.saturating_add(index))
    }
}

#[cfg(test)]
mod tests {
    use crate::column_number::ColumnNumber;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_first_column_is_number_one() {
        assert_eq!(ColumnNumber::of_index(0).to_string(), "1");
        assert_eq!(ColumnNumber::of_index(2).to_string(), "3");
    }
}

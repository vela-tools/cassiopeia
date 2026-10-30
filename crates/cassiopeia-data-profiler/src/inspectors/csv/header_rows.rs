use serde::Serialize;

/// How many CSV records, below the preamble, make up a table's header.
///
/// One record is the common case. A stacked header spreads each column's label over several records,
/// such as a name, an aggregation, and a unit; the records are joined per column into one field name.
/// The count is in logical CSV records, like [`crate::inspectors::csv::preamble::Preamble`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HeaderRows(usize);

impl HeaderRows {
    /// No header: every record is data, and columns are keyed by position.
    pub const NONE: HeaderRows = HeaderRows(0);

    /// A single header record.
    pub const ONE: HeaderRows = HeaderRows(1);

    /// A header of `records` CSV records.
    #[must_use]
    pub const fn new(records: usize) -> HeaderRows {
        HeaderRows(records)
    }

    /// The number of CSV records the header covers.
    #[must_use]
    pub const fn records(self) -> usize {
        self.0
    }

    /// Whether the table has a header at all.
    #[must_use]
    pub const fn is_present(self) -> bool {
        self.0 > 0
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::header_rows::HeaderRows;
    use pretty_assertions::assert_eq;

    #[test]
    fn no_header_is_absent_and_one_or_more_rows_are_present() {
        assert!(!HeaderRows::NONE.is_present());
        assert!(HeaderRows::ONE.is_present());
        assert!(HeaderRows::new(3).is_present());
    }

    #[test]
    fn header_rows_serialise_as_their_record_count() {
        assert_eq!(serde_json::to_value(HeaderRows::new(3)).unwrap(), serde_json::json!(3));
    }
}

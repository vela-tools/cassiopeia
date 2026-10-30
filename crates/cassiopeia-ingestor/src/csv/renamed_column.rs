use crate::csv::{column_key::ColumnKey, column_number::ColumnNumber};

/// A CSV column read by a key other than the one its header asks for, because another column
/// already holds that key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenamedColumn {
    /// The key the column's header asks for: its cleaned header, or its zero-based position when
    /// the header is blank.
    pub wanted: ColumnKey,
    /// The column that keeps `wanted`.
    pub holder: ColumnNumber,
    /// The renamed column.
    pub column: ColumnNumber,
    /// The key the renamed column is read by instead.
    pub key: ColumnKey,
}

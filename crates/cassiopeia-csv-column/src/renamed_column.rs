use crate::{column_key::ColumnKey, column_number::ColumnNumber};
use derive_more::Display;

/// A CSV column read by a key other than the one its header asks for, because another column
/// already holds that key.
///
/// It renders as the sentence a reader needs to address the column from a mapping.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("column {column} `{wanted}` is read as `{key}`, as column {holder} keeps `{wanted}`")]
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

#[cfg(test)]
mod tests {
    use crate::{column_key::ColumnKey, column_number::ColumnNumber, renamed_column::RenamedColumn};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_renamed_column_renders_both_columns_and_both_keys() {
        let renamed = RenamedColumn {
            wanted: ColumnKey::from("value".to_owned()),
            holder: ColumnNumber::of_index(1),
            column: ColumnNumber::of_index(2),
            key: ColumnKey::from("value@3".to_owned()),
        };

        assert_eq!(renamed.to_string(), "column 3 `value` is read as `value@3`, as column 2 keeps `value`");
    }
}

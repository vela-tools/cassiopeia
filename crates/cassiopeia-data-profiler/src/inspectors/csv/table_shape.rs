/// How many columns a candidate dialect must split a sample into for its table to count.
///
/// A delimiter that splits no record is no evidence of a delimiter at all, so a table of one column
/// is read only once no candidate reads a table of several: a wrong delimiter reads every file as a
/// single column, and that reading would otherwise compete with the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableShape {
    /// A table of more than one column, the reading every candidate is tried for first.
    MultiColumn,
    /// A table of exactly one column, read when no candidate reads a multi-column table.
    SingleColumn,
}

impl TableShape {
    /// Whether a table of `columns` columns has this shape.
    pub const fn fits(self, columns: usize) -> bool {
        match self {
            TableShape::MultiColumn => columns > 1,
            TableShape::SingleColumn => columns == 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::table_shape::TableShape;

    #[test]
    fn a_multi_column_shape_takes_two_columns_or_more() {
        assert!(!TableShape::MultiColumn.fits(1));
        assert!(TableShape::MultiColumn.fits(2));
        assert!(TableShape::MultiColumn.fits(30));
    }

    #[test]
    fn a_single_column_shape_takes_exactly_one_column() {
        assert!(TableShape::SingleColumn.fits(1));
        assert!(!TableShape::SingleColumn.fits(0));
        assert!(!TableShape::SingleColumn.fits(2));
    }
}

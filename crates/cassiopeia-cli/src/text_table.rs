use std::iter;
use tabular::{Row, Table};

/// The space between two columns.
const GAP: &str = "  ";

/// Renders a table of `N` columns: a heading row of `headings`, then `rows` in the order given, each
/// cell left-aligned and padded to the width of its column, so the columns align in a terminal.
///
/// No line ends with space, and the table ends without a line break, as a command prints it with
/// one.
pub(crate) fn render_table<const N: usize>(headings: [&str; N], rows: impl IntoIterator<Item = [String; N]>) -> String {
    let mut table = Table::new(&iter::repeat_n("{:<}", N).collect::<Vec<_>>().join(GAP));
    table.add_row(headings.into_iter().fold(Row::new(), Row::with_cell));
    for row in rows {
        table.add_row(row.into_iter().fold(Row::new(), Row::with_cell));
    }
    table.to_string().lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use crate::text_table::render_table;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_column_is_padded_to_its_widest_cell() {
        let rows = [
            ["a".to_owned(), "bb".to_owned(), "c".to_owned()],
            ["dddd".to_owned(), "e".to_owned(), "ffffff".to_owned()],
        ];

        assert_eq!(render_table(["one", "two", "three"], rows), "one   two  three\na     bb   c\ndddd  e    ffffff");
    }

    #[test]
    fn an_empty_last_cell_leaves_no_trailing_space() {
        assert_eq!(render_table(["one", "two"], [["a".to_owned(), String::new()]]), "one  two\na");
    }

    #[test]
    fn a_table_without_rows_is_its_heading_row() {
        assert_eq!(render_table(["one", "two"], []), "one  two");
    }
}

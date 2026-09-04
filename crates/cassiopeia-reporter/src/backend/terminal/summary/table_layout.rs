use cassiopeia_terminal_style::{paint::paint, palette::FRAME, rendering::Rendering};
use tabular::{Row, Table};

/// Builds a header row from column titles.
pub(crate) fn header_row(titles: &[&str]) -> Row {
    titles.iter().fold(Row::new(), Row::with_cell)
}

/// Renders a table to lines and dims the first (header) row, leaving the data rows plain. The dim
/// styling is applied after layout, so it never perturbs `tabular`'s width computation.
pub(crate) fn dim_header(table: &Table, rendering: Rendering) -> Vec<String> {
    let mut lines: Vec<String> = table.to_string().lines().map(str::to_string).collect();
    if let Some(header) = lines.first_mut() {
        *header = paint(FRAME, header, rendering);
    }
    lines
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::summary::table_layout::{dim_header, header_row};
    use cassiopeia_terminal_style::rendering::Rendering;
    use tabular::{Row, Table};

    const ESCAPE: char = '\u{1b}';

    fn table() -> Table {
        let mut table = Table::new("  {:<}  {:>}");
        table.add_row(header_row(&["reason", "count"]));
        table.add_row(Row::new().with_cell("broker-entity-rejected").with_cell("142"));
        table
    }

    #[test]
    fn a_plain_table_keeps_its_header_unstyled() {
        let lines = dim_header(&table(), Rendering::Plain);

        assert_eq!(lines[0], "  reason                  count");
        assert!(!lines[0].contains(ESCAPE));
    }

    #[test]
    fn a_coloured_table_dims_only_the_header() {
        let lines = dim_header(&table(), Rendering::Colored);

        assert!(lines[0].contains(ESCAPE));
        assert!(!lines[1].contains(ESCAPE));
    }
}

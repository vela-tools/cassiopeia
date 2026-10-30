use crate::inspectors::csv::{
    data_type::DataType,
    header::{is_label_row, typed_evidence},
    table::Table,
};
use ::csv::StringRecord;
use std::collections::HashSet;

/// The fewest typed columns a dense title's header row must break before the row above it is read
/// as a title. One broken column is as likely a first data row whose only typed value is missing.
const MIN_BROKEN_COLUMNS: usize = 2;

/// Counts the leading records of `table` that are titles above its header rather than part of it,
/// for records as wide as the table, which the field-count preamble check cannot see.
///
/// Two shapes are recognised. A caption record fills at most one cell with text and leaves the
/// rest blank, as a spreadsheet export pads a title to the table's width (`Air quality;;;`); a run
/// of captions directly above a label row of at least two cells is a title block. A dense title
/// fills its cells but sits above a header row that breaks the typed columns below it the way only a
/// header does: every cell a distinct label, none blank, and at least [`MIN_BROKEN_COLUMNS`] typed
/// columns carrying a label and none a value of their type. A dense title over an all-text table
/// gives no such evidence and stays part of the table.
///
/// At least one data record is always left below the header.
pub fn title_rows(table: &Table) -> usize {
    let captions = table.records.iter().take_while(|record| is_caption(record)).count();
    let captions = match table.records.get(captions) {
        Some(header) if captions > 0 && is_label_row(header) && filled_cells(header) >= 2 => captions,
        Some(_) | None => 0,
    };
    let dense = usize::from(is_dense_title_over_header(table, captions));
    let titles = captions + dense;

    if table.records.len() > titles + 1 { titles } else { 0 }
}

/// Whether `record` fills at most one cell, with text rather than a typed value.
fn is_caption(record: &StringRecord) -> bool {
    filled_cells(record) <= 1 && record.iter().all(|cell| matches!(DataType::detect(cell), DataType::Text | DataType::Empty))
}

/// Whether record `row` of `table` is a title above a header row that only a header could be.
fn is_dense_title_over_header(table: &Table, row: usize) -> bool {
    let Some(header) = table.records.get(row + 1) else {
        return false;
    };
    if table.records.len() <= row + 2 || !is_label_row(header) || filled_cells(header) != header.len() || !has_distinct_cells(header) {
        return false;
    }

    let evidence = typed_evidence(table, row + 1);
    evidence.conforming == 0 && evidence.broken >= MIN_BROKEN_COLUMNS
}

/// The number of non-blank cells in `record`.
fn filled_cells(record: &StringRecord) -> usize {
    record.iter().filter(|cell| !cell.trim().is_empty()).count()
}

/// Whether no two non-blank cells of `record` hold the same text.
fn has_distinct_cells(record: &StringRecord) -> bool {
    let mut seen = HashSet::new();
    record.iter().map(str::trim).filter(|cell| !cell.is_empty()).all(|cell| seen.insert(cell))
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{table::Table, title::title_rows};
    use cassiopeia_csv_dialect::{
        character_encoding::CharacterEncoding,
        dialect::Dialect,
        dialect_byte::DialectByte,
        escape::Escape,
        header_rows::HeaderRows,
        line_terminator::LineTerminator,
        preamble::Preamble,
        quote::Quote,
        value_notation::ValueNotation,
    };
    use encoding_rs::UTF_8;
    use pretty_assertions::assert_eq;

    fn table(sample: &str) -> Table {
        let dialect = Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::NONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        };
        Table::parse(sample.as_bytes(), &dialect).unwrap()
    }

    #[test]
    fn a_padded_title_above_a_header_is_one_title_row() {
        assert_eq!(title_rows(&table("Air quality report;;\nStation;NO2;O3\nSTEF;12,45;NE\nAKA;9,80;40,10\n")), 1);
    }

    #[test]
    fn a_block_of_padded_titles_and_a_spacer_row_are_all_title_rows() {
        assert_eq!(
            title_rows(&table("Air quality report;;\nGenerated 25.10.26;;\n;;\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n")),
            3
        );
    }

    #[test]
    fn a_padded_title_above_a_stacked_header_is_one_title_row() {
        assert_eq!(
            title_rows(&table("Lumes Wien;;;\n;Zeit-NO2;NO2;NO2\n;;HMW;MW24\nSTEF;30.10.2026, 10:00;18,48;12,1\n")),
            1
        );
    }

    #[test]
    fn a_dense_title_above_a_header_over_numeric_columns_is_one_title_row() {
        assert_eq!(title_rows(&table("Lumes;v2.10;25.10.26\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n")), 1);
    }

    #[test]
    fn a_dense_title_over_an_all_text_table_is_not_detected() {
        assert_eq!(title_rows(&table("Lumes;v2.10;25.10.26\nStation;NO2;O3\nSTEF;12,45;NE\nAKA;9,80;40,10\n")), 0);
    }

    #[test]
    fn a_header_above_numeric_data_has_no_title_rows() {
        assert_eq!(title_rows(&table("Station;NO2;O3\nSTEF;12;54\nAKA;9;40\nTAB;7;38\n")), 0);
    }

    #[test]
    fn a_first_data_row_of_repeated_missing_markers_does_not_make_the_header_a_title() {
        assert_eq!(title_rows(&table("station;no2;o3\nTAB;NE;NE\nSTEF;12;54\nAKA;9;40\n")), 0);
    }

    #[test]
    fn a_unit_row_below_the_header_does_not_make_the_header_a_title() {
        assert_eq!(title_rows(&table("Station;NO2;O3\n;ppb;ppb\nSTEF;12;54\nAKA;9;40\n")), 0);
    }

    #[test]
    fn a_sparse_first_row_of_headerless_numeric_data_is_not_a_title() {
        assert_eq!(title_rows(&table("5;;\n6;7;8\n9;1;2\n")), 0);
    }

    #[test]
    fn a_padded_title_with_no_data_below_the_header_is_not_a_title() {
        assert_eq!(title_rows(&table("Air quality report;;\nStation;NO2;O3\n")), 0);
    }
}

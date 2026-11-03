use crate::inspectors::csv::{table::Table, table_shape::TableShape, title::title_rows};
use cassiopeia_csv_dialect::{dialect::Dialect, preamble::Preamble};

/// The most leading records a preamble may cover. It bounds the candidate preambles tried per
/// dialect; real title blocks above a table are a line or a few.
const MAX_PREAMBLE_RECORDS: u64 = 10;

/// The fewest records a table below a non-empty preamble must hold: a header and a data row. A
/// shorter tail is more likely a coincidence of a wrong delimiter than a table.
const MIN_RECORDS_BELOW_PREAMBLE: usize = 2;

/// Where the table of `shape` in `sample` starts under `candidate`, when no preamble is stated:
/// below the preamble and the titles detected, with the headerless table from there.
pub fn detected_table_start(sample: &[u8], candidate: Dialect, shape: TableShape) -> Option<(Dialect, Table)> {
    let (dialect, table) = locate_table(sample, candidate, shape)?;
    Some(below_titles(sample, dialect, table))
}

/// Finds where the table of `shape` in `sample` starts under `dialect`'s delimiter and quote,
/// returning the dialect with its preamble set and the headerless table below that preamble.
///
/// A preamble is the run of leading records, such as a title line, whose field count differs
/// from the table's. The reader is not flexible, so while such a record is still in front of the
/// table the parse fails; the shortest preamble under which it succeeds is the one detected. A
/// candidate preamble is only tried when every record in it is a different width from the record
/// that would start the table, so a ragged row further down never turns the rows above it, the
/// header among them, into a preamble.
fn locate_table(sample: &[u8], mut dialect: Dialect, shape: TableShape) -> Option<(Dialect, Table)> {
    let widths: Vec<usize> = dialect
        .reader_builder()
        .flexible(true)
        .from_reader(sample)
        .records()
        .zip(0..=MAX_PREAMBLE_RECORDS)
        .map_while(|(record, _)| record.ok())
        .map(|record| record.len())
        .collect();

    for ((index, width), records) in widths.iter().enumerate().zip(0_u64..) {
        if widths[..index].contains(width) {
            continue;
        }
        dialect.preamble = Preamble::new(records);
        if let Some(table) = Table::parse(sample, &dialect) {
            // Every record from here on shares one width, so a longer preamble could only drop
            // rows of the same table, never reveal a different one.
            let enough_rows = records == 0 || table.num_rows >= MIN_RECORDS_BELOW_PREAMBLE;
            return (table.is_tabular(shape) && enough_rows).then_some((dialect, table));
        }
    }
    None
}

/// Extends `dialect`'s preamble over the title records at the top of `table` that are as wide as
/// the table, returning the dialect and the headerless table below them. A dialect whose table
/// starts with no such title, or whose preamble is already at [`MAX_PREAMBLE_RECORDS`], is
/// returned unchanged.
fn below_titles(sample: &[u8], mut dialect: Dialect, table: Table) -> (Dialect, Table) {
    let preamble = dialect.preamble;
    let Ok(titles) = u64::try_from(title_rows(&table)) else {
        return (dialect, table);
    };
    let titles = titles.min(MAX_PREAMBLE_RECORDS.saturating_sub(preamble.records()));
    if titles == 0 {
        return (dialect, table);
    }

    dialect.preamble = Preamble::new(preamble.records().saturating_add(titles));
    if let Some(below) = Table::parse(sample, &dialect) {
        return (dialect, below);
    }
    dialect.preamble = preamble;
    (dialect, table)
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{table_shape::TableShape, table_start::detected_table_start};
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

    fn semicolon() -> Dialect {
        Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::NONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }
    }

    fn preamble_of(sample: &[u8]) -> Option<Preamble> {
        detected_table_start(sample, semicolon(), TableShape::MultiColumn).map(|(dialect, _)| dialect.preamble)
    }

    #[test]
    fn a_table_from_the_first_record_has_no_preamble() {
        assert_eq!(preamble_of(b"Station;NO2;O3\nSTEF;12;54\nAKA;9;40\n"), Some(Preamble::NONE));
    }

    #[test]
    fn a_title_line_of_another_width_is_the_preamble() {
        assert_eq!(preamble_of(b"Lumes;v2.10\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n"), Some(Preamble::new(1)));
    }

    #[test]
    fn a_padded_title_as_wide_as_the_table_joins_the_preamble() {
        assert_eq!(
            preamble_of(b"Lumes;v2.10\nAir quality report;;\nStation;NO2;O3\nSTEF;12;54\nAKA;9;40\n"),
            Some(Preamble::new(2))
        );
    }

    #[test]
    fn a_ragged_row_below_the_header_never_turns_the_header_into_a_preamble() {
        assert_eq!(preamble_of(b"Station;NO2;O3\nSTEF;12\nAKA;9;40\nTAB;7;38\n"), Some(Preamble::NONE));
    }

    #[test]
    fn a_preamble_over_a_lone_record_is_no_table() {
        assert_eq!(preamble_of(b"Lumes;v2.10\nStation;NO2;O3\n"), None);
    }
}

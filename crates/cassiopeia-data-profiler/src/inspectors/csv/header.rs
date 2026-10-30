use crate::inspectors::csv::{
    data_type::DataType,
    table::{Table, to_f64},
};
use ::csv::StringRecord;
use std::collections::{HashMap, HashSet};

/// The longest a cell can be and still read as a field label rather than a value.
const MAX_LABEL_LENGTH: usize = 40;

/// The most whitespace-separated words a field label carries; more reads as prose, i.e. a value.
const MAX_LABEL_WORDS: usize = 3;

/// The most records a stacked header may add below its first record. Real stacked headers carry a
/// qualifier or two, such as an aggregation and a unit.
const MAX_CONTINUATION_ROWS: usize = 3;

/// Scores how much row 0 of `table` looks like a header, as a signed value in `[-1, 1]`: positive
/// is header-like, negative is data-like.
///
/// The primary signal is a column-pattern violation. Each column's type is learned from the rows
/// below row 0; row 0 is then header-like where it breaks a well-typed column (a text label over
/// a numeric or boolean column) and data-like where it conforms to one (a number sitting in a
/// numeric column). This is what separates a real header from a data row that merely happens to
/// be mostly text: a data row's first cells still fit their columns' types.
///
/// When no column carries a strong type (an all-text table), type gives no signal and the score
/// falls back to row 0's shape: a header's cells are usually unique, non-blank textual labels.
pub fn header_quality(table: &Table) -> f64 {
    let Some(first_row) = table.records.first() else {
        return 0.0;
    };
    if first_row.is_empty() || table.num_columns == 0 {
        return 0.0;
    }

    let evidence = typed_evidence(table, 0);
    if evidence.typed_columns > 0 {
        return (to_f64(evidence.broken) - to_f64(evidence.conforming)) / to_f64(table.num_columns);
    }

    header_shape(first_row)
}

/// How one record of a table sits against the typed columns below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedEvidence {
    /// The columns whose cells below the record carry a strong, non-textual type.
    pub typed_columns: usize,
    /// The typed columns where the record holds a text label, as a header does.
    pub broken: usize,
    /// The typed columns where the record holds a value of the column's own type, as a data row does.
    pub conforming: usize,
}

/// Measures record `row` of `table` against the type each column takes in the records below it.
pub fn typed_evidence(table: &Table, row: usize) -> TypedEvidence {
    let mut evidence = TypedEvidence {
        typed_columns: 0,
        broken: 0,
        conforming: 0,
    };
    for column in &table.column_types {
        let Some(dominant) = dominant_typed_pattern(column.get(row + 1..).unwrap_or(&[])) else {
            continue;
        };
        evidence.typed_columns += 1;
        match column.get(row).copied().unwrap_or(DataType::Empty) {
            // A text label sitting over a numeric or boolean column is header evidence.
            DataType::Text | DataType::Empty => evidence.broken += 1,
            // A boolean literal (`on`, `off`, `yes`, `no`, `true`, `false`, `y`, `n`) is a bare
            // word that is equally a valid column label, so a record holding one over a boolean
            // column is genuinely ambiguous and must not be scored as a data row — a header named
            // `on` otherwise reads as data and the whole entity renders with a null id. It gives
            // no signal either way.
            DataType::Boolean => {}
            // The record fits the column's own type, so it reads as one more data row.
            cell if cell == dominant => evidence.conforming += 1,
            // Typed but a different type: inconclusive, contributes nothing.
            DataType::Integer
            | DataType::Float
            | DataType::Date
            | DataType::Time
            | DataType::DateTime
            | DataType::Email
            | DataType::Url
            | DataType::Phone
            | DataType::Currency
            | DataType::Percentage => {}
        }
    }
    evidence
}

/// Whether row 0 of a headerless `table` should still be read as a header: every cell is a label
/// and no cell a value, over a table with at least one typed data column.
pub fn looks_like_header_row(table: &Table) -> bool {
    if table.num_rows < 2 || table.column_types.is_empty() {
        return false;
    }

    // Row 0 must read as field labels rather than values on two counts: no cell carries a genuine
    // data value, and every cell is label-shaped (short and comma-free, so a multi-word textual
    // value is not mistaken for a header either).
    //
    // A boolean literal (`on`, `off`, `yes`, `no`, `true`, `false`, `y`, `n`) is a bare word that
    // is equally a valid column label, so a boolean-typed row-0 cell counts as a label here, not a
    // value. Without this a header column legitimately named `on` types as a boolean, row 0 is
    // read as data, the file is ingested headerless, and every name-keyed field reference misses
    // (the entity renders with a null id).
    let Some(first_row) = table.records.first() else {
        return false;
    };
    let row0_carries_no_value = table
        .column_types
        .iter()
        .all(|col| col.first().is_none_or(|t| matches!(t, DataType::Text | DataType::Empty | DataType::Boolean)));
    let row0_all_labels = !first_row.is_empty()
        && first_row.iter().all(|cell| {
            let cell = cell.trim();
            cell.is_empty() || is_field_label(cell)
        });
    if !row0_carries_no_value || !row0_all_labels {
        return false;
    }

    table
        .column_types
        .iter()
        .any(|col| col.iter().skip(1).any(|t| !matches!(t, DataType::Text | DataType::Empty)))
}

/// Counts the records directly below a header that continue it rather than start the data, in
/// `rows`, the records after the first header record.
///
/// A continuation record qualifies the header's columns, such as an aggregation (`HMW`) or a unit
/// (`µg/m³`). Two traits separate it from data. Every non-blank cell reads as a label, and no cell
/// carries a typed value. And it is sparse where the data is dense: it leaves blank at least one
/// column that every data record below the header fills, as a stacked header leaves its key
/// column's label blank. An all-text table of short values has no such column, so its first data
/// records are never taken for header. At least one data record must remain below the header.
pub fn continuation_rows(rows: &[StringRecord]) -> usize {
    let labelled = rows.iter().take(MAX_CONTINUATION_ROWS).take_while(|row| is_label_row(row)).count();
    let Some(data) = rows.get(labelled..).filter(|data| !data.is_empty()) else {
        return 0;
    };

    rows[..labelled].iter().take_while(|row| leaves_dense_column_blank(row, data)).count()
}

/// Whether every non-blank cell of `row` is a field label and none carries a typed value.
pub fn is_label_row(row: &StringRecord) -> bool {
    row.iter()
        .map(str::trim)
        .all(|cell| cell.is_empty() || (is_field_label(cell) && matches!(DataType::detect(cell), DataType::Text | DataType::Boolean)))
}

/// Whether `row` is blank in some column that every record of `data` fills.
fn leaves_dense_column_blank(row: &StringRecord, data: &[StringRecord]) -> bool {
    row.iter()
        .enumerate()
        .any(|(column, cell)| cell.trim().is_empty() && data.iter().all(|record| record.get(column).is_some_and(|value| !value.trim().is_empty())))
}

/// The dominant type of a column's data cells, but only when that type is a strong, non-textual
/// pattern the header check can measure row 0 against.
///
/// `Empty` cells are ignored as missing values. The dominant type must cover at least four fifths of
/// the remaining cells, so a noisy or mixed column contributes no signal; a column that is dominantly
/// `Text` or has no typed cells returns `None`.
fn dominant_typed_pattern(cells: &[DataType]) -> Option<DataType> {
    let mut counts: HashMap<DataType, usize> = HashMap::new();
    let mut present = 0usize;
    for &cell in cells {
        if cell == DataType::Empty {
            continue;
        }
        present += 1;
        *counts.entry(cell).or_insert(0) += 1;
    }

    if present == 0 {
        return None;
    }

    let (dominant, count) = counts.into_iter().max_by_key(|&(_, count)| count)?;
    if matches!(dominant, DataType::Text | DataType::Empty) {
        return None;
    }

    (to_f64(count) / to_f64(present) >= 0.8).then_some(dominant)
}

/// A fallback header signal for an all-text table, where column types cannot separate a header from
/// a data row.
///
/// It rests on shape: a header row's cells look like field labels (short, distinct tokens such as
/// `name` or `iso_code`), whereas a data row carries real values. A single cell that reads as a
/// value rather than a label (one with a comma, a parenthesis, or several words) is taken as proof
/// that row 0 is data, so the score turns negative; an all-label row scores positive, more so when
/// its labels are distinct.
fn header_shape(first_row: &StringRecord) -> f64 {
    let mut seen = HashSet::new();
    let mut unique = true;
    let mut labels = 0usize;
    for field in first_row {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        if !is_field_label(field) {
            return -1.0;
        }
        labels += 1;
        if !seen.insert(field) {
            unique = false;
        }
    }

    if labels == 0 {
        return 0.0;
    }

    if unique { 1.0 } else { 0.5 }
}

/// Whether `cell` has the shape of a field label rather than a data value: short, free of the
/// commas, parentheses, and semicolons a value carries, and no more than a few words.
fn is_field_label(cell: &str) -> bool {
    cell.len() <= MAX_LABEL_LENGTH && !cell.contains([',', '(', ')', ';']) && cell.split_whitespace().count() <= MAX_LABEL_WORDS
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{
        data_type::DataType,
        header::{continuation_rows, dominant_typed_pattern, header_shape, is_field_label},
    };
    use ::csv::StringRecord;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_short_token_is_a_field_label_and_prose_is_not() {
        assert!(is_field_label("iso_code"));
        assert!(!is_field_label("Bonaire, Saint Eustatius"));
        assert!(!is_field_label("one two three four"));
    }

    #[test]
    fn a_mostly_numeric_column_has_a_dominant_typed_pattern() {
        let cells = [DataType::Integer, DataType::Integer, DataType::Integer, DataType::Integer, DataType::Text];

        assert_eq!(dominant_typed_pattern(&cells), Some(DataType::Integer));
    }

    #[test]
    fn a_text_column_has_no_dominant_typed_pattern() {
        assert_eq!(dominant_typed_pattern(&[DataType::Text, DataType::Text]), None);
    }

    #[test]
    fn a_row_of_distinct_labels_scores_as_a_header_shape() {
        assert!((header_shape(&StringRecord::from(vec!["name", "code"])) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_row_holding_a_value_scores_as_data_shape() {
        assert!((header_shape(&StringRecord::from(vec!["Aruba, island", "AW"])) + 1.0).abs() < f64::EPSILON);
    }

    fn records(rows: &[&str]) -> Vec<StringRecord> {
        rows.iter().map(|row| StringRecord::from(row.split(';').collect::<Vec<_>>())).collect()
    }

    #[test]
    fn aggregation_and_unit_rows_below_a_header_are_continuation_rows() {
        let rows = records(&[
            ";;HMW;MW24",
            ";MESZ;µg/m³;µg/m³",
            "STEF;30.10.2026, 10:00;18,48;12,48",
            "TAB;30.10.2026, 10:00;NE;14,03",
        ]);

        assert_eq!(continuation_rows(&rows), 2);
    }

    #[test]
    fn an_all_text_table_of_short_values_has_no_continuation_rows() {
        let rows = records(&["Aruba;AW;AA", "Antigua;AG;AC", "Albania;AL;"]);

        assert_eq!(continuation_rows(&rows), 0);
    }

    #[test]
    fn a_label_row_that_leaves_no_dense_column_blank_is_data() {
        let rows = records(&["STEF;MESZ;NE", "TAB;30.10.2026, 10:00;12,5", "AKA;30.10.2026, 10:00;9,8"]);

        assert_eq!(continuation_rows(&rows), 0);
    }

    #[test]
    fn a_row_holding_a_number_is_not_a_continuation_row() {
        let rows = records(&[";12;HMW", "STEF;30.10.2026, 10:00;18,48"]);

        assert_eq!(continuation_rows(&rows), 0);
    }

    #[test]
    fn label_rows_with_no_data_below_are_not_continuation_rows() {
        assert_eq!(continuation_rows(&records(&[";HMW", ";MESZ"])), 0);
    }
}

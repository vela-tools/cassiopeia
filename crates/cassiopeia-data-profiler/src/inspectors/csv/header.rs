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

    let mut evidence = 0.0;
    let mut typed_columns = 0usize;
    for column in &table.column_types {
        let Some(dominant) = dominant_typed_pattern(column.get(1..).unwrap_or(&[])) else {
            continue;
        };
        typed_columns += 1;
        match column.first().copied().unwrap_or(DataType::Empty) {
            // A text label sitting over a numeric or boolean column is header evidence.
            DataType::Text | DataType::Empty => evidence += 1.0,
            // A boolean literal (`on`, `off`, `yes`, `no`, `true`, `false`, `y`, `n`) is a bare
            // word that is equally a valid column label, so row 0 holding one over a boolean
            // column is genuinely ambiguous and must not be scored as a data row — a header named
            // `on` otherwise reads as data and the whole entity renders with a null id. It gives
            // no signal either way.
            DataType::Boolean => {}
            // Row 0 fits the column's own type, so it reads as one more data row.
            row0 if row0 == dominant => evidence -= 1.0,
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

    if typed_columns > 0 {
        return evidence / to_f64(table.num_columns);
    }

    header_shape(first_row)
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
        header::{dominant_typed_pattern, header_shape, is_field_label},
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
}

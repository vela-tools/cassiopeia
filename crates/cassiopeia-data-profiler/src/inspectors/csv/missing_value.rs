/// The spellings a source writes in a cell for a value it does not have, in place of leaving the
/// cell blank. They are the markers pandas `read_csv` reads as missing by default (its `na_values`),
/// and `NE` (`nicht erhoben`), which Vienna's air-quality feed writes for a value not measured.
/// Matching is exact, so a label such as `Na` (sodium) is no marker.
const MISSING_VALUE_MARKERS: [&str; 19] = [
    "#N/A", "#N/A N/A", "#NA", "-1.#IND", "-1.#QNAN", "-NaN", "-nan", "1.#IND", "1.#QNAN", "<NA>", "N/A", "NA", "NE", "NULL", "NaN", "None", "n/a", "nan",
    "null",
];

/// Whether `cell`, trimmed, spells a missing value rather than a label or a value.
///
/// A marker stands in for a value of whatever type its column holds, so in a typed column it is
/// neither a label breaking the column's type nor a value conforming to it.
pub fn is_missing_value_marker(cell: &str) -> bool {
    MISSING_VALUE_MARKERS.contains(&cell.trim())
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::missing_value::is_missing_value_marker;

    #[test]
    fn the_markers_pandas_reads_as_missing_are_missing_value_markers() {
        assert!(is_missing_value_marker("n/a"));
        assert!(is_missing_value_marker("NULL"));
        assert!(is_missing_value_marker(" NaN "));
    }

    #[test]
    fn the_marker_vienna_writes_for_a_value_not_measured_is_a_missing_value_marker() {
        assert!(is_missing_value_marker("NE"));
    }

    #[test]
    fn a_label_that_only_resembles_a_marker_is_no_missing_value_marker() {
        assert!(!is_missing_value_marker("Na"));
        assert!(!is_missing_value_marker("Station"));
        assert!(!is_missing_value_marker(""));
    }
}

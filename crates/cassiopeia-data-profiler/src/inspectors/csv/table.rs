use crate::inspectors::csv::{data_type::DataType, dialect::Dialect};
use ::csv::StringRecord;
use std::collections::HashMap;

/// A sample parsed under one candidate dialect, with every cell typed for uniformity scoring.
#[derive(Debug)]
pub struct Table {
    /// The parsed records, row 0 included unless the candidate reads it as a header.
    pub records: Vec<StringRecord>,
    /// The detected type of every cell, column by column.
    pub column_types: Vec<Vec<DataType>>,
    /// The field count of the first record.
    pub num_columns: usize,
    /// The number of parsed records.
    pub num_rows: usize,
}

impl Table {
    /// Parses `sample` under `dialect` into a table, skipping its preamble and, when the dialect
    /// reads one, its header row.
    ///
    /// Returns `None` when the sample holds no records or any record fails to parse. The reader is
    /// not flexible, so a record whose field count differs from the first one fails the parse.
    pub fn parse(sample: &[u8], dialect: &Dialect) -> Option<Table> {
        let records = dialect.reader(sample).ok()?.records().collect::<Result<Vec<_>, _>>().ok()?;
        if records.is_empty() {
            return None;
        }

        let num_columns = records.first().map_or(0, StringRecord::len);
        let num_rows = records.len();
        let column_types = (0..num_columns)
            .map(|col| records.iter().map(|record| record.get(col).map_or(DataType::Empty, DataType::detect)).collect())
            .collect();

        Some(Table {
            records,
            column_types,
            num_columns,
            num_rows,
        })
    }

    /// Whether the table has the shape of tabular data at all: more than one column and a row.
    pub const fn is_tabular(&self) -> bool {
        self.num_columns > 1 && self.num_rows > 0
    }

    /// Scores how coherent the table is, independent of whether row 0 is a header. The header
    /// decision is made separately and weighed in by the caller.
    pub fn score(&self) -> f64 {
        let mut score = 0.0;
        score += self.column_consistency() * 40.0;
        score += self.type_uniformity() * 30.0;
        let col_bonus = to_f64(self.num_columns).min(20.0) / 20.0;
        score += col_bonus * 10.0;
        if self.num_rows <= 1 {
            score -= 20.0;
        }

        let empty_cols = self.column_types.iter().filter(|col| col.iter().all(|t| *t == DataType::Empty)).count();
        if self.num_columns > 0 {
            score -= (to_f64(empty_cols) / to_f64(self.num_columns)) * 15.0;
        }

        // A field still wrapped in matching quotes means the quote candidate under test left the
        // enclosing quotes in the data. The other scoring terms cannot see this: a quoted numeric
        // column reads as uniform `Text` whether or not the quotes were stripped, so quoting-off
        // would otherwise tie or beat quoting-on for every file with quoted numeric columns. This
        // penalty is what makes the correct quote character win.
        score -= self.enclosed_quote_fraction() * 50.0;
        score
    }

    /// Fraction of non-empty parsed fields still enclosed in a matching pair of quote characters.
    fn enclosed_quote_fraction(&self) -> f64 {
        let mut total = 0usize;
        let mut enclosed = 0usize;
        for record in &self.records {
            for field in record {
                let field = field.trim();
                if field.is_empty() {
                    continue;
                }
                total += 1;
                if is_enclosed_in_quotes(field) {
                    enclosed += 1;
                }
            }
        }

        if total == 0 { 0.0 } else { to_f64(enclosed) / to_f64(total) }
    }

    fn column_consistency(&self) -> f64 {
        if self.records.is_empty() {
            return 0.0;
        }
        let expected = self.num_columns;
        let consistent = self.records.iter().filter(|r| r.len() == expected).count();
        to_f64(consistent) / to_f64(self.records.len())
    }

    fn type_uniformity(&self) -> f64 {
        if self.column_types.is_empty() {
            return 0.0;
        }

        let mut total_uniformity = 0.0;
        for col_types in &self.column_types {
            let non_empty: Vec<&DataType> = col_types.iter().filter(|t| **t != DataType::Empty).collect();
            if non_empty.is_empty() {
                continue;
            }
            let mut type_counts: HashMap<&DataType, usize> = HashMap::new();
            for t in &non_empty {
                *type_counts.entry(t).or_insert(0) += 1;
            }
            let max_count = type_counts.values().max().copied().unwrap_or(0);
            total_uniformity += to_f64(max_count) / to_f64(non_empty.len());
        }

        total_uniformity / to_f64(self.column_types.len())
    }
}

/// Converts a count to `f64` for ratio scoring, saturating at `u32::MAX`.
pub fn to_f64(count: usize) -> f64 {
    u32::try_from(count).map_or_else(|_| f64::from(u32::MAX), f64::from)
}

/// Whether `field` begins and ends with the same quote character (`"` or `'`).
///
/// A leading apostrophe in ordinary text (`L'Aquila`) is not matched, because only a field that is
/// enclosed on both ends signals unstripped quoting rather than an incidental quote.
fn is_enclosed_in_quotes(field: &str) -> bool {
    let bytes = field.as_bytes();
    matches!(bytes, [first, .., last] if (*first == b'"' || *first == b'\'') && first == last)
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{
        dialect::{Dialect, LineTerminator},
        preamble::Preamble,
        table::{Table, is_enclosed_in_quotes},
    };
    use pretty_assertions::assert_eq;

    fn semicolon_dialect(preamble: Preamble) -> Dialect {
        Dialect {
            delimiter: b';',
            quote_char: Some(b'"'),
            preamble,
            has_headers: false,
            escape: None,
            terminator: LineTerminator::Lf,
            encoding: "UTF-8".to_owned(),
        }
    }

    #[test]
    fn a_title_line_of_another_width_fails_the_parse_without_a_preamble() {
        let sample = b"Lumes;v2.10;25.10.26\nStation;NO2\nSTEF;12,45\n";

        assert!(Table::parse(sample, &semicolon_dialect(Preamble::NONE)).is_none());
    }

    #[test]
    fn a_title_line_skipped_as_preamble_leaves_the_table_below_it() {
        let sample = b"Lumes;v2.10;25.10.26\nStation;NO2\nSTEF;12,45\n";
        let table = Table::parse(sample, &semicolon_dialect(Preamble::new(1))).unwrap();

        assert_eq!(table.num_columns, 2);
        assert_eq!(table.num_rows, 2);
        assert_eq!(table.records[0], vec!["Station", "NO2"]);
    }

    #[test]
    fn an_empty_sample_parses_to_no_table() {
        assert!(Table::parse(b"", &semicolon_dialect(Preamble::NONE)).is_none());
    }

    #[test]
    fn a_single_column_table_is_not_tabular() {
        let table = Table::parse(b"a\nb\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(!table.is_tabular());
    }

    #[test]
    fn only_a_field_enclosed_on_both_ends_counts_as_quoted() {
        assert!(is_enclosed_in_quotes("\"30\""));
        assert!(is_enclosed_in_quotes("'30'"));
        assert!(!is_enclosed_in_quotes("L'Aquila"));
        assert!(!is_enclosed_in_quotes("\""));
    }

    #[test]
    fn unstripped_quotes_lower_the_score() {
        let quoted = Table::parse(
            b"\"1\";\"2\"\n\"3\";\"4\"\n",
            &Dialect {
                quote_char: None,
                ..semicolon_dialect(Preamble::NONE)
            },
        )
        .unwrap();
        let stripped = Table::parse(b"\"1\";\"2\"\n\"3\";\"4\"\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(stripped.score() > quoted.score());
    }
}

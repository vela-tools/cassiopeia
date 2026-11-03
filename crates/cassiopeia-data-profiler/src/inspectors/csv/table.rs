use crate::inspectors::csv::{data_type::DataType, sentence::is_sentence, table_shape::TableShape};
use ::csv::StringRecord;
use cassiopeia_csv_dialect::{dialect::Dialect, misshapen_row::MisshapenRow};
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
    /// The number of header records the dialect read above the parsed records.
    pub header_records: usize,
    /// The number of records skipped because their field count differs from the table's.
    pub misshapen_rows: usize,
}

impl Table {
    /// Parses `sample` under `dialect` into a table, skipping its preamble and, when the dialect
    /// reads one, its header row.
    ///
    /// A record whose field count differs from the first one is skipped as a [`MisshapenRow`], as
    /// the ingestor skips it, and counted against the table's consistency. Returns `None` when the
    /// sample holds no records, a record fails to parse for any other reason, or the misshapen
    /// records are not outnumbered by the rest: then the first record is no row of the table, such
    /// as a title line above it, and the table starts further down.
    pub fn parse(sample: &[u8], dialect: &Dialect) -> Option<Table> {
        let mut records = Vec::new();
        let mut misshapen_rows = 0usize;
        for result in dialect.reader(sample).ok()?.records() {
            match result {
                Ok(record) => records.push(record),
                Err(error) => {
                    MisshapenRow::of(&error, dialect.preamble)?;
                    misshapen_rows += 1;
                }
            }
        }
        let records = records;
        if records.len() <= misshapen_rows {
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
            header_records: dialect.header_rows.records(),
            misshapen_rows,
        })
    }

    /// Whether the table is tabular data of `shape`: a row, and as many columns as the shape takes.
    ///
    /// A single column is also no table when most of its cells are sentences. Every candidate
    /// reads plain prose as one column, a line to a record, so that column is a text, not a list.
    pub fn is_tabular(&self, shape: TableShape) -> bool {
        let fits = shape.fits(self.num_columns) && self.num_rows > 0;
        match shape {
            TableShape::MultiColumn => fits,
            TableShape::SingleColumn => fits && !self.reads_as_prose(),
        }
    }

    /// Whether more than half of the table's non-blank cells are sentences.
    fn reads_as_prose(&self) -> bool {
        let cells = self.records.iter().flatten().filter(|cell| !cell.trim().is_empty());
        let (sentences, total) = cells.fold((0usize, 0usize), |(sentences, total), cell| {
            (sentences + usize::from(is_sentence(cell)), total + 1)
        });
        sentences * 2 > total
    }

    /// Scores how coherent the table is, independent of whether row 0 is a header. The header
    /// decision is made separately and weighed in by the caller.
    pub fn score(&self) -> f64 {
        let mut score = 0.0;
        score += self.column_consistency() * 40.0;
        score += self.type_uniformity() * 30.0;
        let col_bonus = to_f64(self.num_columns).min(20.0) / 20.0;
        score += col_bonus * 10.0;
        // A candidate that reads the sample as a single record has most likely swallowed it whole,
        // such as a wrong quote spanning every line break. A header over one data row is two
        // records, so the header records count too, or the header hypothesis of a table with one
        // data row would always lose to the headerless reading of the same records.
        if self.num_rows + self.header_records <= 1 {
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

    /// The fraction of the sample's records that have the table's field count.
    fn column_consistency(&self) -> f64 {
        let total = self.num_rows + self.misshapen_rows;
        if total == 0 { 0.0 } else { to_f64(self.num_rows) / to_f64(total) }
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
        table::{Table, is_enclosed_in_quotes},
        table_shape::TableShape,
    };
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

    fn semicolon_dialect(preamble: Preamble) -> Dialect {
        Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble,
            header_rows: HeaderRows::NONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        }
    }

    #[test]
    fn a_title_line_of_another_width_fails_the_parse_without_a_preamble() {
        let sample = b"Lumes;v2.10;25.10.26\nStation;NO2\nSTEF;12,45\n";

        assert!(Table::parse(sample, &semicolon_dialect(Preamble::NONE)).is_none());
    }

    #[test]
    fn a_short_and_a_long_row_are_skipped_and_the_rows_around_them_kept() {
        let sample = b"1;a;10\n2;b\n3;c;30\n4;d;40;x\n5;e;50\n";
        let table = Table::parse(sample, &semicolon_dialect(Preamble::NONE)).unwrap();

        assert_eq!(table.num_rows, 3);
        assert_eq!(table.misshapen_rows, 2);
        assert_eq!(table.records[1], vec!["3", "c", "30"]);
    }

    #[test]
    fn a_footer_note_below_the_table_is_skipped() {
        let sample = b"1;a;10\n2;b;20\nTotal;;30\nSource: City statistics office\n";
        let table = Table::parse(sample, &semicolon_dialect(Preamble::NONE)).unwrap();

        assert_eq!(table.num_rows, 3);
        assert_eq!(table.misshapen_rows, 1);
    }

    #[test]
    fn misshapen_rows_as_many_as_the_rest_fail_the_parse() {
        assert!(Table::parse(b"1;a;10\n2;b\n3;c;30\n4\n", &semicolon_dialect(Preamble::NONE)).is_none());
    }

    #[test]
    fn misshapen_rows_lower_the_score() {
        let even = Table::parse(b"1;a;10\n2;b;20\n3;c;30\n", &semicolon_dialect(Preamble::NONE)).unwrap();
        let ragged = Table::parse(b"1;a;10\n2;b\n2;b;20\n3;c;30\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(even.score() > ragged.score());
    }

    #[test]
    fn a_header_over_one_data_row_scores_as_the_same_two_records_read_headerless() {
        let sample = b"Station;NO2;O3\nSTEF;12,45;54,55\n";
        let headerless = Table::parse(sample, &semicolon_dialect(Preamble::NONE)).unwrap();
        let headed = Table::parse(
            sample,
            &Dialect {
                header_rows: HeaderRows::ONE,
                ..semicolon_dialect(Preamble::NONE)
            },
        )
        .unwrap();

        assert_eq!(headed.num_rows, 1);
        assert_eq!(headed.header_records, 1);
        assert!((headed.score() - headerless.score()).abs() < f64::EPSILON);
    }

    #[test]
    fn a_single_record_without_a_header_scores_below_the_same_record_over_a_second() {
        let single = Table::parse(b"STEF;12,45;54,55\n", &semicolon_dialect(Preamble::NONE)).unwrap();
        let double = Table::parse(b"STEF;12,45;54,55\nAKA;9,80;40,10\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(single.score() < double.score());
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
    fn a_single_column_table_is_tabular_only_as_a_single_column() {
        let table = Table::parse(b"a\nb\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(!table.is_tabular(TableShape::MultiColumn));
        assert!(table.is_tabular(TableShape::SingleColumn));
    }

    #[test]
    fn a_single_column_of_sentences_is_not_tabular() {
        let table = Table::parse(b"Introduction\nThis is a text.\nMore text here.\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(!table.is_tabular(TableShape::SingleColumn));
    }

    #[test]
    fn a_single_column_with_one_sentence_among_names_is_tabular() {
        let table = Table::parse(b"Ada\nGrace\nThis one is a sentence.\nLinus\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(table.is_tabular(TableShape::SingleColumn));
    }

    #[test]
    fn a_two_column_table_is_tabular_only_as_a_multi_column_one() {
        let table = Table::parse(b"a;b\n1;2\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(table.is_tabular(TableShape::MultiColumn));
        assert!(!table.is_tabular(TableShape::SingleColumn));
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
                quote_char: Quote::Disabled,
                ..semicolon_dialect(Preamble::NONE)
            },
        )
        .unwrap();
        let stripped = Table::parse(b"\"1\";\"2\"\n\"3\";\"4\"\n", &semicolon_dialect(Preamble::NONE)).unwrap();

        assert!(stripped.score() > quoted.score());
    }
}

use crate::inspectors::csv::table::Table;
use cassiopeia_csv_dialect::dialect::Dialect;

/// One reading of a sample: a candidate dialect, the table it reads, and how coherent that table is.
pub struct Reading {
    /// The candidate dialect, with the preamble and header settled for this reading.
    pub dialect: Dialect,
    /// The table the dialect reads from the sample.
    pub table: Table,
    /// How coherent the table is, the header signal included.
    pub score: f64,
}

impl Reading {
    /// The reading of the two that scores higher; `other` must score strictly higher to win.
    pub fn or_better(self, other: Option<Reading>) -> Reading {
        match other {
            Some(other) if other.score > self.score => other,
            Some(_) | None => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::{reading::Reading, table::Table};
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

    fn reading(delimiter: DialectByte, score: f64) -> Reading {
        let dialect = Dialect {
            delimiter,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::NONE,
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(UTF_8),
            notation: ValueNotation::PLAIN,
        };
        let table = Table::parse(b"a,b\n1,2\n", &dialect).unwrap();
        Reading { dialect, table, score }
    }

    #[test]
    fn a_higher_scoring_reading_replaces_the_current_one() {
        let best = reading(DialectByte::COMMA, 10.0).or_better(Some(reading(DialectByte::SEMICOLON, 20.0)));

        assert_eq!(best.dialect.delimiter, DialectByte::SEMICOLON);
    }

    #[test]
    fn a_tie_keeps_the_current_reading() {
        let best = reading(DialectByte::COMMA, 10.0).or_better(Some(reading(DialectByte::SEMICOLON, 10.0)));

        assert_eq!(best.dialect.delimiter, DialectByte::COMMA);
    }

    #[test]
    fn no_other_reading_keeps_the_current_one() {
        assert_eq!(reading(DialectByte::COMMA, 10.0).or_better(None).dialect.delimiter, DialectByte::COMMA);
    }
}

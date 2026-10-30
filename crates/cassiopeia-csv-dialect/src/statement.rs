use crate::{
    character_encoding::CharacterEncoding,
    decimal_separator::DecimalSeparator,
    dialect::Dialect,
    dialect_byte::DialectByte,
    error::DialectError,
    escape::Escape,
    header_rows::HeaderRows,
    header_statement::HeaderStatement,
    line_terminator::LineTerminator,
    null_token::NullToken,
    preamble::Preamble,
    quote::Quote,
    value_notation::ValueNotation,
};
use serde::{Deserialize, Serialize};

/// What a manifest states about a CSV source's dialect: its `dialect` object.
///
/// Every structural member is optional. A stated member wins, and detection still settles the ones
/// left out; a statement of every structural member is a whole [`Dialect`], so detection does not
/// run at all. The value notation, the decimal separator and the null tokens, is never detected, so
/// it is whatever is stated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DialectStatementRaw", into = "DialectStatementRaw")]
pub struct DialectStatement {
    delimiter: Option<DialectByte>,
    quote: Option<Quote>,
    escape: Option<Escape>,
    skip_rows: Option<Preamble>,
    header: Option<HeaderStatement>,
    terminator: Option<LineTerminator>,
    encoding: Option<CharacterEncoding>,
    notation: ValueNotation,
}

impl DialectStatement {
    /// A statement of nothing: every member is left to detection.
    pub const NOTHING: DialectStatement = DialectStatement {
        delimiter: None,
        quote: None,
        escape: None,
        skip_rows: None,
        header: None,
        terminator: None,
        encoding: None,
        notation: ValueNotation::PLAIN,
    };

    /// The stated field delimiter.
    #[must_use]
    pub const fn delimiter(&self) -> Option<DialectByte> {
        self.delimiter
    }

    /// The stated quoting.
    #[must_use]
    pub const fn quote(&self) -> Option<Quote> {
        self.quote
    }

    /// The stated escape of a quote inside a quoted field.
    #[must_use]
    pub const fn escape(&self) -> Option<Escape> {
        self.escape
    }

    /// The stated number of preamble records above the table (`skipRows`).
    #[must_use]
    pub const fn skip_rows(&self) -> Option<Preamble> {
        self.skip_rows
    }

    /// The stated header (`header` and `headerRows`).
    #[must_use]
    pub const fn header(&self) -> Option<HeaderStatement> {
        self.header
    }

    /// The stated line terminator.
    #[must_use]
    pub const fn terminator(&self) -> Option<LineTerminator> {
        self.terminator
    }

    /// The stated character encoding.
    #[must_use]
    pub const fn encoding(&self) -> Option<CharacterEncoding> {
        self.encoding
    }

    /// The stated value notation, plain when nothing about it is stated.
    #[must_use]
    pub const fn notation(&self) -> &ValueNotation {
        &self.notation
    }

    /// The dialect this statement describes on its own, when it states every structural member, so
    /// the file is read without detecting anything. A stated `header: true` leaves the header's
    /// record count to detection, so it is not a whole dialect.
    ///
    /// The escape is the one structural member a whole dialect need not state: with nothing left to
    /// detect, an unstated escape is RFC 4180's doubling (clause 2.7), which is also how a dialect
    /// that doubles its quotes states itself.
    #[must_use]
    pub fn whole_dialect(&self) -> Option<Dialect> {
        let Some(HeaderStatement::Rows(header_rows)) = self.header else {
            return None;
        };
        Some(Dialect {
            delimiter: self.delimiter?,
            quote_char: self.quote?,
            preamble: self.skip_rows?,
            header_rows,
            escape: self.escape.unwrap_or_default(),
            terminator: self.terminator?,
            encoding: self.encoding?,
            // The dialect owns its notation, while the statement stays the manifest's.
            notation: self.notation.clone(),
        })
    }
}

/// States every member of `dialect`, so a detected dialect can be written into a manifest as is.
impl From<&Dialect> for DialectStatement {
    fn from(dialect: &Dialect) -> DialectStatement {
        DialectStatement {
            delimiter: Some(dialect.delimiter),
            quote: Some(dialect.quote_char),
            // Doubling is RFC 4180's escape and what an unstated escape reads as in a whole dialect,
            // so it is left out, as is any escape under disabled quoting, where it has no effect.
            escape: match (dialect.quote_char, dialect.escape) {
                (Quote::Char(_), Escape::Char(escape)) => Some(Escape::Char(escape)),
                (Quote::Char(_) | Quote::Disabled, Escape::Doubled) | (Quote::Disabled, Escape::Char(_)) => None,
            },
            skip_rows: Some(dialect.preamble),
            header: Some(HeaderStatement::Rows(dialect.header_rows)),
            terminator: Some(dialect.terminator),
            encoding: Some(dialect.encoding),
            // The statement owns its notation, while the dialect stays the profile's.
            notation: dialect.notation.clone(),
        }
    }
}

/// The on-the-wire shape of a `dialect` object, before its members are checked against each other.
///
/// `header` and `headerRows` are two members for one fact, so they are kept apart here and resolved
/// into one [`HeaderStatement`]. A member left at its plain value is not written back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DialectStatementRaw {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delimiter: Option<DialectByte>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quote: Option<Quote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    escape: Option<Escape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skip_rows: Option<Preamble>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    header: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    header_rows: Option<HeaderRows>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminator: Option<LineTerminator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encoding: Option<CharacterEncoding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decimal_separator: Option<DecimalSeparator>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    null_values: Vec<NullToken>,
}

impl TryFrom<DialectStatementRaw> for DialectStatement {
    type Error = DialectError;

    fn try_from(raw: DialectStatementRaw) -> Result<DialectStatement, DialectError> {
        if let (Some(delimiter), Some(Quote::Char(quote))) = (raw.delimiter, raw.quote)
            && delimiter == quote
        {
            return Err(DialectError::DelimiterIsQuote(char::from(delimiter.byte())));
        }
        if let (Some(_), Some(Quote::Disabled)) = (raw.escape, raw.quote) {
            return Err(DialectError::EscapeWithoutQuote);
        }
        if let (Some(Escape::Char(escape)), Some(Quote::Char(quote))) = (raw.escape, raw.quote)
            && escape == quote
        {
            return Err(DialectError::EscapeIsQuote(char::from(escape.byte())));
        }

        Ok(DialectStatement {
            delimiter: raw.delimiter,
            quote: raw.quote,
            escape: raw.escape,
            skip_rows: raw.skip_rows,
            header: HeaderStatement::from_members(raw.header, raw.header_rows)?,
            terminator: raw.terminator,
            encoding: raw.encoding,
            notation: ValueNotation {
                decimal_separator: raw.decimal_separator.unwrap_or_default(),
                null_values: raw.null_values,
            },
        })
    }
}

impl From<DialectStatement> for DialectStatementRaw {
    fn from(statement: DialectStatement) -> DialectStatementRaw {
        let (header, header_rows) = statement.header.map_or((None, None), HeaderStatement::members);
        let decimal_separator = statement.notation.decimal_separator;

        DialectStatementRaw {
            delimiter: statement.delimiter,
            quote: statement.quote,
            escape: statement.escape,
            skip_rows: statement.skip_rows,
            header,
            header_rows,
            terminator: statement.terminator,
            encoding: statement.encoding,
            decimal_separator: (decimal_separator != DecimalSeparator::default()).then_some(decimal_separator),
            null_values: statement.notation.null_values,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        character_encoding::CharacterEncoding,
        decimal_separator::DecimalSeparator,
        dialect::Dialect,
        dialect_byte::DialectByte,
        error::DialectError,
        escape::Escape,
        header_rows::HeaderRows,
        header_statement::HeaderStatement,
        line_terminator::LineTerminator,
        null_token::NullToken,
        preamble::Preamble,
        quote::Quote,
        statement::DialectStatement,
        value_notation::ValueNotation,
    };
    use encoding_rs::WINDOWS_1252;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> Result<DialectStatement, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn an_empty_object_states_nothing() {
        assert_eq!(parse(json!({})).unwrap(), DialectStatement::NOTHING);
        assert_eq!(DialectStatement::default(), DialectStatement::NOTHING);
    }

    #[test]
    fn the_issue_example_reads_every_member() {
        let statement = parse(json!({
            "delimiter": ";",
            "encoding": "latin1",
            "skipRows": 1,
            "headerRows": 3,
            "decimalSeparator": ",",
            "nullValues": ["NE", "---"],
        }))
        .unwrap();

        assert_eq!(statement.delimiter(), Some(DialectByte::SEMICOLON));
        assert_eq!(statement.encoding(), Some(CharacterEncoding::new(WINDOWS_1252)));
        assert_eq!(statement.skip_rows(), Some(Preamble::new(1)));
        assert_eq!(statement.header(), Some(HeaderStatement::Rows(HeaderRows::new(3))));
        assert_eq!(statement.quote(), None);
        assert_eq!(statement.terminator(), None);
        assert_eq!(statement.notation().decimal_separator, DecimalSeparator::Comma);
        assert!(statement.notation().is_null_token("NE"));
        assert!(statement.notation().is_null_token("---"));
    }

    #[test]
    fn an_unknown_member_is_rejected() {
        assert!(parse(json!({"separator": ";"})).is_err());
    }

    #[test]
    fn a_delimiter_equal_to_the_quote_is_rejected() {
        let error = parse(json!({"delimiter": "'", "quote": "'"})).unwrap_err();

        assert!(error.to_string().contains("both '''"));
    }

    #[test]
    fn a_header_flag_contradicting_the_row_count_is_rejected() {
        let error = parse(json!({"header": false, "headerRows": 2})).unwrap_err();

        assert!(error.to_string().contains("'header' is false"));
    }

    #[test]
    fn a_partial_statement_is_no_whole_dialect() {
        assert_eq!(parse(json!({"delimiter": ";", "skipRows": 1})).unwrap().whole_dialect(), None);
    }

    #[test]
    fn a_header_left_to_detection_is_no_whole_dialect() {
        let statement = parse(json!({"delimiter": ";", "quote": "\"", "skipRows": 0, "header": true, "terminator": "lf", "encoding": "utf-8"})).unwrap();

        assert_eq!(statement.whole_dialect(), None);
    }

    #[test]
    fn a_statement_of_every_structural_member_is_a_whole_dialect() {
        let statement = parse(json!({
            "delimiter": ";",
            "quote": "none",
            "skipRows": 1,
            "headerRows": 3,
            "terminator": "crlf",
            "encoding": "latin1",
            "nullValues": ["NE"],
        }))
        .unwrap();
        let dialect = statement.whole_dialect().unwrap();

        assert_eq!(dialect.delimiter, DialectByte::SEMICOLON);
        assert_eq!(dialect.quote_char, Quote::Disabled);
        assert_eq!(dialect.preamble, Preamble::new(1));
        assert_eq!(dialect.header_rows, HeaderRows::new(3));
        assert_eq!(dialect.terminator, LineTerminator::Crlf);
        assert_eq!(dialect.encoding, CharacterEncoding::new(WINDOWS_1252));
        assert!(dialect.notation.is_null_token("NE"));
    }

    #[test]
    fn a_dialect_states_itself_in_the_member_names_a_manifest_reads() {
        let dialect = Dialect {
            delimiter: DialectByte::SEMICOLON,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::new(1),
            header_rows: HeaderRows::new(3),
            escape: Escape::Doubled,
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(WINDOWS_1252),
            notation: ValueNotation::PLAIN,
        };
        let json = serde_json::to_value(DialectStatement::from(&dialect)).unwrap();

        assert_eq!(
            json,
            json!({"delimiter": ";", "quote": "\"", "skipRows": 1, "headerRows": 3, "terminator": "lf", "encoding": "windows-1252"})
        );
        assert_eq!(parse(json).unwrap().whole_dialect().unwrap().header_rows, HeaderRows::new(3));
    }

    #[test]
    fn a_headerless_dialect_states_header_false() {
        let statement = parse(json!({"header": false})).unwrap();

        assert_eq!(serde_json::to_value(&statement).unwrap(), json!({"header": false}));
    }

    #[test]
    fn a_stated_notation_round_trips() {
        let statement = parse(json!({"decimalSeparator": ",", "nullValues": ["NE"]})).unwrap();

        assert_eq!(
            serde_json::to_value(&statement).unwrap(),
            json!({"decimalSeparator": ",", "nullValues": ["NE"]})
        );
        assert_eq!(statement.notation().null_values, vec![NullToken::try_from("NE".to_owned()).unwrap()]);
    }

    #[test]
    fn a_stated_escape_is_read_and_round_trips() {
        for (stated, escape) in [("\\", Escape::Char(DialectByte::BACKSLASH)), ("double", Escape::Doubled)] {
            let statement = parse(json!({"quote": "\"", "escape": stated})).unwrap();

            assert_eq!(statement.escape(), Some(escape));
            assert_eq!(serde_json::to_value(&statement).unwrap(), json!({"quote": "\"", "escape": stated}));
        }
    }

    #[test]
    fn a_stated_escape_is_rejected_when_quoting_is_off() {
        for stated in ["\\", "double"] {
            let error = serde_json::from_value::<DialectStatement>(json!({"quote": "none", "escape": stated})).unwrap_err();

            assert_eq!(error.to_string(), DialectError::EscapeWithoutQuote.to_string());
        }
    }

    #[test]
    fn an_escape_equal_to_the_quote_is_rejected() {
        let error = parse(json!({"quote": "'", "escape": "'"})).unwrap_err();

        assert_eq!(error.to_string(), DialectError::EscapeIsQuote('\'').to_string());
    }

    #[test]
    fn a_whole_statement_with_a_stated_escape_is_a_dialect_with_that_escape() {
        let statement = parse(json!({
            "delimiter": ",",
            "quote": "\"",
            "escape": "\\",
            "skipRows": 0,
            "headerRows": 1,
            "terminator": "lf",
            "encoding": "utf-8",
        }))
        .unwrap();

        assert_eq!(statement.whole_dialect().unwrap().escape, Escape::Char(DialectByte::BACKSLASH));
    }

    #[test]
    fn a_dialect_states_an_escape_only_when_it_is_not_doubling() {
        let dialect = Dialect {
            delimiter: DialectByte::COMMA,
            quote_char: Quote::Char(DialectByte::DOUBLE_QUOTE),
            preamble: Preamble::NONE,
            header_rows: HeaderRows::ONE,
            escape: Escape::Char(DialectByte::BACKSLASH),
            terminator: LineTerminator::Lf,
            encoding: CharacterEncoding::new(WINDOWS_1252),
            notation: ValueNotation::PLAIN,
        };
        let doubled = Dialect {
            escape: Escape::Doubled,
            ..dialect.clone()
        };

        assert_eq!(serde_json::to_value(DialectStatement::from(&dialect)).unwrap()["escape"], json!("\\"));
        assert!(serde_json::to_value(DialectStatement::from(&doubled)).unwrap().get("escape").is_none());
    }
}

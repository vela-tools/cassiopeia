use crate::error::DialectError;
use serde::{Deserialize, Serialize};

/// A byte with a structural role in a CSV record, a delimiter or a quote: one ASCII character other
/// than a line break, which ends a record rather than structuring one. A manifest states it as a
/// one-character string, such as `";"` or `"\t"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DialectByte(u8);

impl DialectByte {
    /// The comma, RFC 4180's delimiter.
    pub const COMMA: DialectByte = DialectByte(b',');

    /// The horizontal tab, the delimiter of tab-separated values.
    pub const TAB: DialectByte = DialectByte(b'\t');

    /// The semicolon, the usual delimiter where the comma is the decimal separator.
    pub const SEMICOLON: DialectByte = DialectByte(b';');

    /// The vertical bar.
    pub const PIPE: DialectByte = DialectByte(b'|');

    /// The space.
    pub const SPACE: DialectByte = DialectByte(b' ');

    /// The double quote, RFC 4180's quote.
    pub const DOUBLE_QUOTE: DialectByte = DialectByte(b'"');

    /// The single quote.
    pub const SINGLE_QUOTE: DialectByte = DialectByte(b'\'');

    /// The byte itself, as the CSV parser takes it.
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }
}

impl TryFrom<String> for DialectByte {
    type Error = DialectError;

    fn try_from(text: String) -> Result<DialectByte, DialectError> {
        match text.as_bytes() {
            &[byte] if byte.is_ascii() && byte != b'\n' && byte != b'\r' => Ok(DialectByte(byte)),
            _ => Err(DialectError::InvalidByte(text)),
        }
    }
}

// Not derivable: the byte is written back as the one-character string a manifest states it as.
impl From<DialectByte> for String {
    fn from(byte: DialectByte) -> String {
        char::from(byte.0).to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::{dialect_byte::DialectByte, error::DialectError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_single_ascii_character_is_a_dialect_byte() {
        assert_eq!(DialectByte::try_from(";".to_owned()), Ok(DialectByte::SEMICOLON));
        assert_eq!(DialectByte::try_from("\t".to_owned()), Ok(DialectByte::TAB));
    }

    #[test]
    fn several_characters_a_non_ascii_character_and_a_line_break_are_rejected() {
        for text in ["", ";;", "µ", "\n", "\r"] {
            assert_eq!(DialectByte::try_from(text.to_owned()), Err(DialectError::InvalidByte(text.to_owned())));
        }
    }

    #[test]
    fn a_dialect_byte_round_trips_as_a_one_character_string() {
        let json = serde_json::to_value(DialectByte::TAB).unwrap();

        assert_eq!(json, serde_json::json!("\t"));
        assert_eq!(serde_json::from_value::<DialectByte>(json).unwrap(), DialectByte::TAB);
    }
}

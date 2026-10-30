use crate::{dialect_byte::DialectByte, error::DialectError};
use serde::{Deserialize, Serialize};

/// The word a manifest states to turn quoting off.
const DISABLED: &str = "none";

/// How the fields of a CSV record are quoted: by one quote character, or not at all, so a quote
/// character in a field is literal text. A manifest states a one-character string, or `"none"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Quote {
    /// Fields may be enclosed in this character (RFC 4180 clause 2.5).
    Char(DialectByte),
    /// No character quotes a field.
    Disabled,
}

impl Quote {
    /// The quote byte, or `None` when quoting is off.
    #[must_use]
    pub const fn byte(self) -> Option<u8> {
        match self {
            Quote::Char(quote) => Some(quote.byte()),
            Quote::Disabled => None,
        }
    }
}

impl TryFrom<String> for Quote {
    type Error = DialectError;

    fn try_from(text: String) -> Result<Quote, DialectError> {
        if text == DISABLED {
            Ok(Quote::Disabled)
        } else {
            DialectByte::try_from(text).map(Quote::Char)
        }
    }
}

// Not derivable: a quote is written back as the string a manifest states it as.
impl From<Quote> for String {
    fn from(quote: Quote) -> String {
        match quote {
            Quote::Char(byte) => String::from(byte),
            Quote::Disabled => DISABLED.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{dialect_byte::DialectByte, quote::Quote};
    use pretty_assertions::assert_eq;

    #[test]
    fn none_turns_quoting_off_and_a_character_quotes() {
        assert_eq!(serde_json::from_str::<Quote>(r#""none""#).unwrap(), Quote::Disabled);
        assert_eq!(serde_json::from_str::<Quote>(r#""'""#).unwrap(), Quote::Char(DialectByte::SINGLE_QUOTE));
    }

    #[test]
    fn a_quote_of_several_characters_is_rejected() {
        assert!(serde_json::from_str::<Quote>(r#""''""#).is_err());
    }

    #[test]
    fn a_quote_round_trips_as_the_string_it_was_stated_as() {
        assert_eq!(serde_json::to_value(Quote::Disabled).unwrap(), serde_json::json!("none"));
        assert_eq!(serde_json::to_value(Quote::Char(DialectByte::DOUBLE_QUOTE)).unwrap(), serde_json::json!("\""));
    }

    #[test]
    fn the_quote_byte_is_absent_when_quoting_is_off() {
        assert_eq!(Quote::Disabled.byte(), None);
        assert_eq!(Quote::Char(DialectByte::DOUBLE_QUOTE).byte(), Some(b'"'));
    }
}

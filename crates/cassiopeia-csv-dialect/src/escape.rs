use crate::{dialect_byte::DialectByte, error::DialectError};
use serde::{Deserialize, Serialize};

/// The word a manifest states for quotes escaped by doubling.
const DOUBLED: &str = "double";

/// How a quote character inside a quoted field is escaped: by doubling it, or by a byte written in
/// front of it. A manifest states `"double"`, or a one-character string such as `"\\"`.
///
/// Doubling is read under either: a doubled quote inside a quoted field is a literal quote in both.
/// The escape byte acts only inside a quoted field, where it is dropped and the byte after it is
/// read literally, whatever that byte is, so `\\` reads as `\` and `\n` as `n`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Escape {
    /// A quote inside a quoted field is written twice (RFC 4180 clause 2.7).
    #[default]
    Doubled,
    /// A quote inside a quoted field follows this byte, as MySQL's `SELECT ... INTO OUTFILE` and
    /// PHP's `fputcsv` write it with a backslash.
    Char(DialectByte),
}

impl Escape {
    /// The escape byte, or `None` when quotes are escaped by doubling.
    #[must_use]
    pub const fn byte(self) -> Option<u8> {
        match self {
            Escape::Doubled => None,
            Escape::Char(escape) => Some(escape.byte()),
        }
    }
}

impl TryFrom<String> for Escape {
    type Error = DialectError;

    fn try_from(text: String) -> Result<Escape, DialectError> {
        if text == DOUBLED {
            Ok(Escape::Doubled)
        } else {
            DialectByte::try_from(text).map(Escape::Char)
        }
    }
}

// Not derivable: an escape is written back as the string a manifest states it as.
impl From<Escape> for String {
    fn from(escape: Escape) -> String {
        match escape {
            Escape::Doubled => DOUBLED.to_owned(),
            Escape::Char(byte) => String::from(byte),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{dialect_byte::DialectByte, escape::Escape};
    use pretty_assertions::assert_eq;

    #[test]
    fn double_escapes_by_doubling_and_a_character_escapes_by_that_character() {
        assert_eq!(serde_json::from_str::<Escape>(r#""double""#).unwrap(), Escape::Doubled);
        assert_eq!(serde_json::from_str::<Escape>(r#""\\""#).unwrap(), Escape::Char(DialectByte::BACKSLASH));
    }

    #[test]
    fn an_escape_of_several_characters_is_rejected() {
        assert!(serde_json::from_str::<Escape>(r#""\\\\""#).is_err());
    }

    #[test]
    fn an_escape_round_trips_as_the_string_it_was_stated_as() {
        assert_eq!(serde_json::to_value(Escape::Doubled).unwrap(), serde_json::json!("double"));
        assert_eq!(serde_json::to_value(Escape::Char(DialectByte::BACKSLASH)).unwrap(), serde_json::json!("\\"));
    }

    #[test]
    fn the_escape_byte_is_absent_when_quotes_are_doubled() {
        assert_eq!(Escape::Doubled.byte(), None);
        assert_eq!(Escape::Char(DialectByte::BACKSLASH).byte(), Some(b'\\'));
    }

    #[test]
    fn quotes_are_doubled_by_default() {
        assert_eq!(Escape::default(), Escape::Doubled);
    }
}

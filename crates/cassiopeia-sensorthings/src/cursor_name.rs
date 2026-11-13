use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The most characters a cursor name holds.
const MAX_LENGTH: usize = 64;

/// The name of a cursor, the store that keeps the read positions of one incremental input, such as
/// `rivm-air`.
///
/// A name is 1 to 64 characters of `a-z`, `0-9` and `-`, and neither starts nor ends with `-`. It
/// names the store's file, so it is a file name on every platform, and the same in a file system
/// that folds letter case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct CursorName(Box<str>);

/// Why a text is no cursor name.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CursorNameError {
    /// The text is empty.
    #[error("a cursor name holds 1 to 64 characters, not none")]
    Empty,

    /// The text is longer than 64 characters.
    #[error("a cursor name holds 1 to 64 characters, not {0}")]
    TooLong(usize),

    /// The text holds a character other than `a-z`, `0-9` and `-`.
    #[error("a cursor name holds only `a-z`, `0-9` and `-`, not `{0}`")]
    Character(char),

    /// The text starts or ends with `-`.
    #[error("a cursor name neither starts nor ends with `-`")]
    EdgeHyphen,
}

impl CursorName {
    /// The name as written.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CursorName {
    type Error = CursorNameError;

    fn try_from(text: String) -> Result<CursorName, CursorNameError> {
        let length = text.chars().count();
        if length == 0 {
            return Err(CursorNameError::Empty);
        }
        if length > MAX_LENGTH {
            return Err(CursorNameError::TooLong(length));
        }
        if let Some(character) = text.chars().find(|character| !matches!(character, 'a'..='z' | '0'..='9' | '-')) {
            return Err(CursorNameError::Character(character));
        }
        if text.starts_with('-') || text.ends_with('-') {
            return Err(CursorNameError::EdgeHyphen);
        }
        Ok(CursorName(text.into_boxed_str()))
    }
}

// The command line hands a name over as borrowed text, and the check is the one `TryFrom<String>`
// makes for a manifest, which no derive expresses.
impl FromStr for CursorName {
    type Err = CursorNameError;

    /// Reads a name written as text, such as on the command line.
    fn from_str(text: &str) -> Result<CursorName, CursorNameError> {
        CursorName::try_from(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use crate::cursor_name::{CursorName, CursorNameError};
    use pretty_assertions::assert_eq;

    fn name(text: &str) -> Result<CursorName, CursorNameError> {
        CursorName::try_from(text.to_owned())
    }

    #[test]
    fn malformed_cursor_names_are_refused() {
        for (text, refusal) in [
            (String::new(), CursorNameError::Empty),
            ("Rivm".to_owned(), CursorNameError::Character('R')),
            ("-x".to_owned(), CursorNameError::EdgeHyphen),
            ("x-".to_owned(), CursorNameError::EdgeHyphen),
            ("a".repeat(65), CursorNameError::TooLong(65)),
            ("a_b".to_owned(), CursorNameError::Character('_')),
        ] {
            assert_eq!(name(&text), Err(refusal), "{text}");
        }
    }

    #[test]
    fn well_formed_cursor_names_are_kept() {
        for text in ["rivm-air".to_owned(), "a".to_owned(), "a".repeat(64), "pm10-2026".to_owned()] {
            assert_eq!(name(&text).unwrap().as_str(), text);
        }
    }

    #[test]
    fn a_cursor_name_is_written_and_read_as_its_text() {
        let name: CursorName = serde_json::from_str(r#""rivm-air""#).unwrap();

        assert_eq!(name.to_string(), "rivm-air");
        assert_eq!(serde_json::to_string(&name).unwrap(), r#""rivm-air""#);
    }

    #[test]
    fn borrowed_text_is_checked_as_owned_text_is() {
        assert_eq!("rivm-air".parse::<CursorName>().unwrap().as_str(), "rivm-air");
        assert_eq!("Rivm".parse::<CursorName>(), Err(CursorNameError::Character('R')));
    }

    #[test]
    fn a_refused_cursor_name_names_why() {
        let error = serde_json::from_str::<CursorName>(r#""rivm air""#).unwrap_err();

        assert!(error.to_string().contains("not ` `"), "{error}");
    }
}

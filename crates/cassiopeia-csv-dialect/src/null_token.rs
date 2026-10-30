use crate::error::DialectError;
use derive_more::{AsRef, Into};
use serde::{Deserialize, Serialize};

/// A field value a CSV file writes for a missing value, such as `NE` or `---`.
///
/// Fields are compared after their surrounding whitespace is trimmed, so the token is held trimmed
/// too. A blank token is refused, since a blank field already reads as missing.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Into, Serialize, Deserialize)]
#[as_ref(str)]
#[serde(try_from = "String", into = "String")]
pub struct NullToken(String);

impl TryFrom<String> for NullToken {
    type Error = DialectError;

    fn try_from(text: String) -> Result<NullToken, DialectError> {
        match text.trim() {
            "" => Err(DialectError::BlankNullToken),
            trimmed if trimmed.len() == text.len() => Ok(NullToken(text)),
            trimmed => Ok(NullToken(trimmed.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::DialectError, null_token::NullToken};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_token_is_held_trimmed() {
        let token = NullToken::try_from(" NE ".to_owned()).unwrap();

        assert_eq!(token.as_ref(), "NE");
    }

    #[test]
    fn a_blank_token_is_rejected() {
        assert_eq!(NullToken::try_from("  ".to_owned()), Err(DialectError::BlankNullToken));
    }

    #[test]
    fn a_token_round_trips_as_a_string() {
        let token: NullToken = serde_json::from_str(r#""---""#).unwrap();

        assert_eq!(serde_json::to_value(&token).unwrap(), serde_json::json!("---"));
    }
}

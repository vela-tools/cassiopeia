use crate::{decimal_separator::DecimalSeparator, null_token::NullToken};
use serde::Serialize;

/// How a CSV file writes the values in its fields: the decimal separator of its numbers and the
/// tokens it writes for a missing value. Detection cannot tell either apart from text, so both are
/// only ever stated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValueNotation {
    /// The separator between a number's integer part and its fraction.
    pub decimal_separator: DecimalSeparator,
    /// The field values read as a missing value, besides a blank field.
    pub null_values: Vec<NullToken>,
}

impl ValueNotation {
    /// The notation of a file that states nothing: full-stop decimals, and no null tokens.
    pub const PLAIN: ValueNotation = ValueNotation {
        decimal_separator: DecimalSeparator::Dot,
        null_values: Vec::new(),
    };

    /// Whether `field`, already trimmed, is one of the stated null tokens.
    #[must_use]
    pub fn is_null_token(&self, field: &str) -> bool {
        self.null_values.iter().any(|token| token.as_ref() == field)
    }
}

#[cfg(test)]
mod tests {
    use crate::{null_token::NullToken, value_notation::ValueNotation};

    #[test]
    fn only_a_stated_token_is_a_null_token() {
        let notation = ValueNotation {
            null_values: vec![NullToken::try_from("NE".to_owned()).unwrap()],
            ..ValueNotation::PLAIN
        };

        assert!(notation.is_null_token("NE"));
        assert!(!notation.is_null_token("NEU"));
        assert!(!ValueNotation::PLAIN.is_null_token("NE"));
    }
}

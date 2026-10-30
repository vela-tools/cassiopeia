use serde::{Deserialize, Serialize};

/// The character that separates a number's integer part from its fraction in a CSV file's values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecimalSeparator {
    /// A full stop, as in `12.45`.
    #[default]
    #[serde(rename = ".")]
    Dot,
    /// A comma, as in `12,45`, the convention across much of continental Europe.
    #[serde(rename = ",")]
    Comma,
}

impl DecimalSeparator {
    /// The separator as the byte it appears as in a field.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            DecimalSeparator::Dot => b'.',
            DecimalSeparator::Comma => b',',
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::decimal_separator::DecimalSeparator;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_separator_is_stated_as_its_character() {
        assert_eq!(serde_json::from_str::<DecimalSeparator>(r#"",""#).unwrap(), DecimalSeparator::Comma);
        assert_eq!(serde_json::to_value(DecimalSeparator::Dot).unwrap(), serde_json::json!("."));
    }

    #[test]
    fn any_other_separator_is_rejected() {
        assert!(serde_json::from_str::<DecimalSeparator>(r#""'""#).is_err());
    }

    #[test]
    fn the_separator_byte_is_its_character() {
        assert_eq!(DecimalSeparator::Comma.byte(), b',');
        assert_eq!(DecimalSeparator::default().byte(), b'.');
    }
}

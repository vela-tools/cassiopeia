use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// The name of an environment variable a template reads, as POSIX spells portable names: a letter
/// or underscore followed by letters, digits, and underscores.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
pub struct VariableName(String);

impl VariableName {
    /// Returns the name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A name that is not a portable environment variable name.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("'{name}' is not a valid environment variable name; use letters, digits and underscores, not starting with a digit")]
pub struct VariableNameError {
    /// The rejected name.
    pub name: String,
}

impl FromStr for VariableName {
    type Err = VariableNameError;

    fn from_str(name: &str) -> Result<VariableName, VariableNameError> {
        let mut characters = name.chars();
        let starts_well = characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
        if starts_well && characters.all(|character| character.is_ascii_alphanumeric() || character == '_') {
            Ok(VariableName(name.to_owned()))
        } else {
            Err(VariableNameError { name: name.to_owned() })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::variable_name::VariableName;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn letters_digits_and_underscores_form_a_name() {
        assert_eq!(VariableName::from_str("API_KEY_2").unwrap().as_str(), "API_KEY_2");
        assert_eq!(VariableName::from_str("_private").unwrap().as_str(), "_private");
    }

    #[test]
    fn a_name_starting_with_a_digit_is_rejected() {
        assert_eq!(VariableName::from_str("2KEY").unwrap_err().name, "2KEY");
    }

    #[test]
    fn an_empty_name_is_rejected() {
        assert!(VariableName::from_str("").is_err());
    }

    #[test]
    fn a_name_with_punctuation_is_rejected() {
        assert!(VariableName::from_str("API-KEY").is_err());
        assert!(VariableName::from_str("API KEY").is_err());
    }
}

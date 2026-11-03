use crate::environment::{
    secret::{MaskStyle, Secret, Secrets},
    variable_name::{VariableName, VariableNameError},
};
use secrecy::{ExposeSecret, SecretString};
use std::{borrow::Cow, env, str::FromStr};
use thiserror::Error;

/// The opening of a placeholder.
const OPENING: &str = "${";

/// The escaped form of a literal `${`.
const ESCAPED_OPENING: &str = "$${";

/// The closing of a placeholder.
const CLOSING: char = '}';

/// One piece of a template: text copied as written, or a variable whose value takes its place.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// Text copied as written, with every `$${` already turned back into `${`.
    Literal(String),
    /// A variable read from the environment.
    Variable(VariableName),
}

/// A text that names environment variables as `${NAME}`, and writes a literal `${` as `$${`.
///
/// A `$` followed by anything other than `{` or `${` is ordinary text, so a template only changes
/// the meaning of text that opens a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(Vec<Segment>);

/// The reason a template could not be read or resolved.
///
/// No variant carries a variable's value, so a failure never reveals a secret.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TemplateError {
    /// A `${` was never closed by a `}`.
    #[error("a '${{' is never closed by a '}}'; write '$${{' for a literal '${{'")]
    Unterminated,

    /// A placeholder named something that is not a variable name.
    #[error(transparent)]
    InvalidName(#[from] VariableNameError),

    /// A variable the template names is not set.
    #[error("the environment variable '{name}' is not set")]
    Unset {
        /// The variable that is not set.
        name: VariableName,
    },

    /// A variable the template names holds a value that is not valid Unicode.
    #[error("the environment variable '{name}' does not hold valid Unicode")]
    NotUnicode {
        /// The variable whose value is unreadable.
        name: VariableName,
    },
}

/// A template with every variable replaced by its value: the resulting text, held as a secret, and
/// the secrets it was built from.
#[derive(Debug)]
pub struct Resolved {
    /// The text with every placeholder filled.
    text: SecretString,
    /// Every value read into the text, by the variable it came from.
    secrets: Secrets,
}

impl Resolved {
    /// Returns the text with every placeholder filled.
    #[must_use]
    pub const fn text(&self) -> &SecretString {
        &self.text
    }

    /// Returns the secrets the text was built from.
    #[must_use]
    pub const fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// Consumes the resolution, returning the secrets the text was built from.
    #[must_use]
    pub fn into_secrets(self) -> Secrets {
        self.secrets
    }
}

impl Template {
    /// Returns whether the template names any variable.
    #[must_use]
    pub fn reads_environment(&self) -> bool {
        self.0.iter().any(|segment| matches!(segment, Segment::Variable(_)))
    }

    /// Fills every placeholder with the value `lookup` gives its variable.
    ///
    /// # Errors
    /// Returns whatever `lookup` fails with for the first variable it cannot read.
    pub fn resolve(&self, lookup: impl Fn(&VariableName) -> Result<SecretString, TemplateError>) -> Result<Resolved, TemplateError> {
        let mut text = String::new();
        let mut secrets = Secrets::new();
        for segment in &self.0 {
            match segment {
                Segment::Literal(literal) => text.push_str(literal),
                Segment::Variable(name) => {
                    let value = lookup(name)?;
                    text.push_str(value.expose_secret());
                    // The secret keeps the name it is masked by, while the segment keeps its own.
                    secrets.insert(Secret::new(name.clone(), &value));
                }
            }
        }
        Ok(Resolved {
            text: SecretString::from(text),
            secrets,
        })
    }
}

/// Reads a variable from this process's environment.
///
/// # Errors
/// Returns [`TemplateError::Unset`] when the variable is not set, and [`TemplateError::NotUnicode`]
/// when its value is not valid Unicode.
pub fn process_environment(name: &VariableName) -> Result<SecretString, TemplateError> {
    match env::var(name.as_str()) {
        Ok(value) => Ok(SecretString::from(value)),
        // The error is rebuilt from the name alone, since the original carries the unreadable value.
        Err(env::VarError::NotPresent) => Err(TemplateError::Unset { name: name.clone() }),
        Err(env::VarError::NotUnicode(_)) => Err(TemplateError::NotUnicode { name: name.clone() }),
    }
}

/// Writes `text` as a template that reads back as exactly `text`, escaping every `${` as `$${`.
#[must_use]
pub fn escape(text: &str) -> Cow<'_, str> {
    Secrets::new().mask(text, MaskStyle::Template)
}

impl FromStr for Template {
    type Err = TemplateError;

    fn from_str(text: &str) -> Result<Template, TemplateError> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(dollar) = rest.find('$') {
            literal.push_str(&rest[..dollar]);
            let from_dollar = &rest[dollar..];
            if let Some(after) = from_dollar.strip_prefix(ESCAPED_OPENING) {
                literal.push_str(OPENING);
                rest = after;
            } else if let Some(after) = from_dollar.strip_prefix(OPENING) {
                let (name, after) = after.split_once(CLOSING).ok_or(TemplateError::Unterminated)?;
                if !literal.is_empty() {
                    segments.push(Segment::Literal(literal));
                    literal = String::new();
                }
                segments.push(Segment::Variable(VariableName::from_str(name)?));
                rest = after;
            } else {
                literal.push('$');
                rest = &from_dollar[1..];
            }
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Ok(Template(segments))
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::{
        template::{Template, TemplateError, escape},
        variable_name::VariableName,
    };
    use pretty_assertions::assert_eq;
    use secrecy::{ExposeSecret, SecretString};
    use std::str::FromStr;

    /// A lookup over a fixed table, standing in for the process environment.
    fn table(entries: &'static [(&'static str, &'static str)]) -> impl Fn(&VariableName) -> Result<SecretString, TemplateError> {
        move |name| {
            entries
                .iter()
                .find(|(key, _)| *key == name.as_str())
                .map(|(_, value)| SecretString::from(*value))
                .ok_or_else(|| TemplateError::Unset { name: name.clone() })
        }
    }

    fn resolve(template: &str, entries: &'static [(&'static str, &'static str)]) -> Result<String, TemplateError> {
        Template::from_str(template)?
            .resolve(table(entries))
            .map(|resolved| resolved.text().expose_secret().to_owned())
    }

    #[test]
    fn a_placeholder_is_filled_with_its_variable() {
        assert_eq!(
            resolve("https://api.example/?apikey=${API_KEY}", &[("API_KEY", "abc")]).unwrap(),
            "https://api.example/?apikey=abc"
        );
    }

    #[test]
    fn several_placeholders_are_filled_in_place() {
        assert_eq!(resolve("${A}-${B}-${A}", &[("A", "1"), ("B", "2")]).unwrap(), "1-2-1");
    }

    #[test]
    fn a_resolution_records_each_variable_once() {
        let resolved = Template::from_str("${A}${B}${A}").unwrap().resolve(table(&[("A", "1"), ("B", "2")])).unwrap();

        assert_eq!(
            resolved.secrets().iter().map(|secret| secret.name().as_str()).collect::<Vec<_>>(),
            vec!["A", "B"]
        );
    }

    #[test]
    fn an_escaped_opening_is_a_literal() {
        assert_eq!(resolve("cost: $${PRICE}", &[]).unwrap(), "cost: ${PRICE}");
        assert!(!Template::from_str("$${PRICE}").unwrap().reads_environment());
    }

    #[test]
    fn a_lone_dollar_is_ordinary_text() {
        assert_eq!(resolve("$5 and $$ and $x", &[]).unwrap(), "$5 and $$ and $x");
    }

    #[test]
    fn text_without_placeholders_reads_no_environment() {
        let template = Template::from_str("https://api.example/").unwrap();

        assert!(!template.reads_environment());
        assert_eq!(resolve("https://api.example/", &[]).unwrap(), "https://api.example/");
    }

    #[test]
    fn an_unset_variable_fails_naming_it() {
        let error = resolve("${MISSING}", &[]).unwrap_err();

        assert_eq!(
            error,
            TemplateError::Unset {
                name: VariableName::from_str("MISSING").unwrap()
            }
        );
        assert!(error.to_string().contains("MISSING"));
    }

    #[test]
    fn an_unclosed_placeholder_is_rejected() {
        assert_eq!(Template::from_str("https://x/${KEY").unwrap_err(), TemplateError::Unterminated);
    }

    #[test]
    fn an_invalid_name_is_rejected() {
        assert!(matches!(Template::from_str("${1KEY}"), Err(TemplateError::InvalidName(_))));
        assert!(matches!(Template::from_str("${}"), Err(TemplateError::InvalidName(_))));
    }

    #[test]
    fn escaping_a_text_makes_it_read_back_as_itself() {
        let text = "https://x/${literal}?a=$$";
        let escaped = escape(text);

        assert_eq!(escaped, "https://x/$${literal}?a=$$");
        assert_eq!(resolve(&escaped, &[]).unwrap(), text);
    }

    #[test]
    fn a_lookup_failure_is_reported_without_the_value() {
        let error = resolve("${A}", &[]).unwrap_err();

        assert_eq!(error.to_string(), "the environment variable 'A' is not set");
    }
}

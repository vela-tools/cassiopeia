use crate::environment::field::TemplatedField;
use cassiopeia_common::{environment::template::TemplateError, error::input::InputError, request_header::RequestHeaderError};
use thiserror::Error;
use url::ParseError;

/// The reason a manifest field could not be read with its environment variables filled in.
///
/// No variant carries a variable's value or a text built from one, so the error never reveals a
/// secret.
#[derive(Debug, Error)]
pub enum EnvironmentError {
    /// The field's template is malformed, or a variable it names cannot be read.
    #[error("{field}: {reason}")]
    Template {
        /// The field the template sits in.
        field: TemplatedField,
        /// What went wrong with the template.
        reason: TemplateError,
    },

    /// An input's source reads the environment but names a local file.
    #[error(
        "{} reads an environment variable but names a local file; environment variables are read only in a URL source",
        TemplatedField::InputSource
    )]
    LocalSource,

    /// An input's source, written without environment variables, does not name a usable source.
    #[error(transparent)]
    Source(#[from] InputError),

    /// A URL field is not a valid URL once its variables are filled in.
    #[error("{field} is not a valid URL: {reason}")]
    InvalidUrl {
        /// The field the URL sits in.
        field: TemplatedField,
        /// Why the URL parser refused it.
        reason: ParseError,
    },

    /// A header's name or filled-in value is not valid HTTP.
    #[error(transparent)]
    Header(#[from] RequestHeaderError),
}

#[cfg(test)]
mod tests {
    use crate::environment::{error::EnvironmentError, field::TemplatedField};
    use cassiopeia_common::environment::{template::TemplateError, variable_name::VariableName};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn an_unset_variable_names_the_variable_and_the_field() {
        let error = EnvironmentError::Template {
            field: TemplatedField::OutputUrl,
            reason: TemplateError::Unset {
                name: VariableName::from_str("BROKER_HOST").unwrap(),
            },
        };

        assert_eq!(error.to_string(), "the output 'url': the environment variable 'BROKER_HOST' is not set");
    }

    #[test]
    fn a_local_source_names_the_field() {
        assert!(
            EnvironmentError::LocalSource
                .to_string()
                .starts_with("an input's 'source' reads an environment variable")
        );
    }
}

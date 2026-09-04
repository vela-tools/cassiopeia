use cassiopeia_common::context::{kind::ContextType, mode::AtContextMode};
use clap::Args;
use std::path::PathBuf;
use thiserror::Error;
use url::Url;

/// A failure raised while validating command-line arguments that clap cannot enforce on its own.
#[derive(Debug, Error)]
pub enum CliValidationError {
    #[error("invalid --context-url")]
    InvalidContextUrl(#[source] url::ParseError),

    #[error("--context-url is required when --context is url")]
    MissingContextUrl,

    #[error("--context-file is required when --context is local")]
    MissingContextFile,

    #[error("--broker-url is required when --writer is context-broker")]
    MissingBrokerUrl,

    #[error("--input is required unless --manifest is given")]
    MissingInput,

    #[error("--mapping is required unless --manifest is given")]
    MissingMapping,
}

/// Arguments controlling how the `@context` is attached to written entities.
#[derive(Args, Debug)]
pub struct ContextArgs {
    /// How to attach the `@context` to written entities.
    #[arg(
        short = 'C',
        long = "context",
        value_enum,
        help_heading = "Context",
        value_name = "MODE",
        default_value_t = ContextType::Default,
    )]
    pub kind: ContextType,

    /// Explicit `@context` URL (required when --context is url).
    #[arg(
        long = "context-url",
        help_heading = "Context",
        value_name = "URL",
        required_if_eq("kind", "url"),
        conflicts_with = "file"
    )]
    pub url: Option<String>,

    /// Local `.jsonld` file to inline as the `@context` (required when --context is local).
    #[arg(
        long = "context-file",
        help_heading = "Context",
        value_name = "FILE",
        required_if_eq("kind", "local"),
        conflicts_with = "url"
    )]
    pub file: Option<PathBuf>,
}

impl ContextArgs {
    /// Resolves the context arguments into an [`AtContextMode`], validating the explicit-URL case.
    ///
    /// clap enforces the `requires`/`conflicts` relationships; this only assembles the value and
    /// parses the URL.
    pub fn resolve(&self) -> Result<Option<AtContextMode>, CliValidationError> {
        match self.kind {
            ContextType::None => Ok(None),
            ContextType::Default => Ok(Some(AtContextMode::Default)),
            ContextType::Url => {
                let raw = self.url.as_deref().ok_or(CliValidationError::MissingContextUrl)?;
                let url = Url::parse(raw).map_err(CliValidationError::InvalidContextUrl)?;
                Ok(Some(AtContextMode::Explicit(url)))
            }
            ContextType::Local => {
                let path = self.file.clone().ok_or(CliValidationError::MissingContextFile)?;
                Ok(Some(AtContextMode::Local(path)))
            }
        }
    }
}

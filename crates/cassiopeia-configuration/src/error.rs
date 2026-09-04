use std::{path::PathBuf, result};
use thiserror::Error;

/// Failures raised while assembling the configuration from its layers.
///
/// `figment2::Error` carries the whole provenance of the failing value and is large enough that
/// leaving it inline would make every `Result` in this crate pay for it, so it is boxed.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read the configuration at '{}'", path.display())]
    Parse { path: PathBuf, source: Box<figment2::Error> },

    #[error("Failed to assemble the configuration from the environment and the built-in defaults")]
    Assemble { source: Box<figment2::Error> },

    #[error("No configuration file exists at '{}'", path.display())]
    NotFound { path: PathBuf },
}

/// The result type every fallible configuration operation returns.
pub type Result<T, E = ConfigError> = result::Result<T, E>;

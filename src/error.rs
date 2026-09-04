use crate::cli::context::CliValidationError;
use cassiopeia_cli::error::CliError;
use cassiopeia_configuration::error::ConfigError;
use cassiopeia_manifest::error::ManifestError;
use cassiopeia_pipeline::error::PipelineError;
use cassiopeia_reporter::error::ReporterError;
use std::result;
use thiserror::Error;

/// A failure raised while wiring or running the command-line surface.
///
/// Each variant carries the typed error of the crate that produced it, so the top-level reporter can
/// render the full source chain.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Cli(#[from] CliError),

    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error(transparent)]
    Pipeline(#[from] PipelineError),

    #[error(transparent)]
    Reporter(#[from] ReporterError),

    #[error(transparent)]
    CliValidation(#[from] CliValidationError),

    #[error("failed to build the worker thread pool")]
    ThreadPool { source: rayon::ThreadPoolBuildError },

    #[error("failed to install the CTRL+C handler")]
    CtrlC(#[from] ctrlc::Error),
}

/// The result type every command-line handler returns, fixing the error type to [`Error`].
pub type Result<T> = result::Result<T, Error>;

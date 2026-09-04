//! Error types for the reporter library.

use indicatif::style::TemplateError;
use std::result;
use thiserror::Error;
use tracing_subscriber::reload::Error as ReloadError;

/// Failures raised while building, installing, or reloading the global reporter.
#[derive(Debug, Error)]
pub enum ReporterError {
    /// The global reporter was already installed by an earlier call.
    #[error("Logger already initialized")]
    AlreadyInitialized,

    /// The tracing subscriber could not be reloaded with a new configuration.
    #[error("Failed to reload the logger")]
    Reload(#[source] ReloadError),

    /// The global worker-guard lock could not be taken.
    #[error("Global state poisoned")]
    Poisoned,

    /// A reporter template could not be parsed into an indicatif progress style.
    #[error("A progress style template is malformed")]
    InvalidStyleTemplate(#[source] TemplateError),

    /// The global worker guard was accessed before the reporter was installed.
    #[error("Global guard not initialized")]
    GuardNotInitialized,
}

/// The result type every fallible reporter operation returns.
pub type Result<T> = result::Result<T, ReporterError>;

#[cfg(test)]
mod tests {
    use crate::error::ReporterError;
    use indicatif::ProgressStyle;
    use std::error::Error;

    #[test]
    fn each_variant_renders_a_distinct_message() {
        assert!(ReporterError::AlreadyInitialized.to_string().contains("already initialized"));
        assert!(ReporterError::GuardNotInitialized.to_string().contains("not initialized"));
        assert!(ReporterError::Poisoned.to_string().contains("poisoned"));
    }

    #[test]
    fn a_malformed_template_keeps_the_parser_failure_as_its_cause() {
        let Err(failure) = ProgressStyle::with_template("{msg:*}") else {
            panic!("the template must be rejected");
        };
        let error = ReporterError::InvalidStyleTemplate(failure);

        assert!(error.source().is_some());
    }
}

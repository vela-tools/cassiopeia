use cassiopeia_mapping::template::error::TemplateError;
use std::result;
use thiserror::Error;

/// Failures raised while extracting an entity's attribute values.
#[derive(Debug, Error)]
pub enum ExtractionError {
    /// Evaluating an attribute's template against the source record failed.
    #[error("An attribute's template could not be resolved")]
    Template {
        #[from]
        source: TemplateError,
    },

    /// A mapping's nested attribute declarations recursed past the guard depth, which points to a
    /// self-referential mapping rather than legitimately deep data.
    #[error("Attribute recursion limit exceeded at depth {depth}")]
    RecursionLimitExceeded {
        /// The depth at which the limit tripped.
        depth: usize,
    },
}

/// The result type used throughout the extraction stage.
pub type Result<T> = result::Result<T, ExtractionError>;

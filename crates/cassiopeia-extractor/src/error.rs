use std::result;
use thiserror::Error;

/// Failures that cost an entity its extraction.
///
/// An attribute whose template fails to render, or whose value the declared transformation refuses,
/// is dropped and recorded rather than failing the entity, so neither appears here.
#[derive(Debug, Error)]
pub enum ExtractionError {
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

#[cfg(test)]
mod tests {
    use crate::error::ExtractionError;

    #[test]
    fn a_recursion_failure_names_the_depth_it_tripped_at() {
        let error = ExtractionError::RecursionLimitExceeded { depth: 51 };

        assert_eq!(error.to_string(), "Attribute recursion limit exceeded at depth 51");
    }
}

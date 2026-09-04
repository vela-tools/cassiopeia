use std::result;
use thiserror::Error;
use urn_rs::Error as UrnParseError;

/// Failures that can arise while building or reading Cassiopeia's intermediate representation.
#[derive(Debug, Error)]
pub enum IrError {
    /// JSON (de)serialization of an intermediate value failed.
    #[error("An intermediate value could not be (de)serialized")]
    SerializationError(#[from] serde_json::Error),

    /// A URN failed to parse or validate.
    #[error("A URN could not be parsed or validated")]
    UrnError(#[from] UrnParseError),
}

/// Shorthand for a `Result` whose error is [`IrError`].
pub type Result<T> = result::Result<T, IrError>;

#[cfg(test)]
mod tests {
    use crate::error::IrError;
    use urn_rs::Urn;

    #[test]
    fn a_urn_parse_failure_converts_into_the_typed_variant() {
        let parse_error = "not-a-urn".parse::<Urn>().expect_err("must reject");
        let error: IrError = parse_error.into();
        assert!(matches!(error, IrError::UrnError(_)));
    }

    #[test]
    fn a_serde_failure_converts_into_the_serialization_variant() {
        let serde_error = serde_json::from_str::<i32>("not json").expect_err("must reject");
        let error: IrError = serde_error.into();
        assert!(matches!(error, IrError::SerializationError(_)));
    }
}

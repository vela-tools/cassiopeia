use cassiopeia_ngsi_ld::entity::error::NgsiLdError;
use std::result;
use thiserror::Error;

/// Failures raised while transforming an entity into its NGSI-LD form.
#[derive(Debug, Error)]
pub enum TransformationError {
    /// Assembling the NGSI-LD entity from its builder failed.
    #[error("The NGSI-LD entity could not be built")]
    Build {
        #[from]
        source: NgsiLdError,
    },
}

/// The result type used throughout the transformation stage.
pub type Result<T> = result::Result<T, TransformationError>;

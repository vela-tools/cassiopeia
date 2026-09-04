use crate::store_action::StoreAction;
use std::{io, path::PathBuf, result};
use thiserror::Error;

/// Errors raised by relationship store operations.
#[derive(Debug, Error)]
pub enum RelationshipStoreError {
    /// A redb database operation failed.
    #[error("Failed to {action} in the relationship store at '{}'", path.display())]
    RedbOperation {
        /// The underlying redb error, boxed because `redb::Error` is large
        /// enough to bloat every `Result` in the crate otherwise.
        #[source]
        source: Box<redb::Error>,
        /// The database path.
        path: PathBuf,
        /// The operation being attempted.
        action: StoreAction,
    },

    /// Failed to create a temporary directory for the on-disk store.
    #[error("Failed to create a temporary directory for the relationship store")]
    CreateTempDirectory {
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A stored composite key could not be parsed back into its components.
    #[error("Invalid relationship key: {key}")]
    InvalidKey {
        /// The rendered key that could not be parsed.
        key: String,
    },
}

/// The result type used throughout relationship store operations.
pub type Result<T> = result::Result<T, RelationshipStoreError>;

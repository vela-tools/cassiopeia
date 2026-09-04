use crate::store_action::StoreAction;
use rmp_serde::{decode, encode};
use std::{io, path::PathBuf, result};
use thiserror::Error;

/// Errors raised by entity store operations.
#[derive(Debug, Error)]
pub enum EntityStoreError {
    /// A redb database operation failed (open, read, write, clear, or iterate).
    #[error("Failed to {action} in the entity store at '{}'", path.display())]
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
    #[error("Failed to create a temporary directory for the entity store")]
    CreateTempDirectory {
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// JSON serialization or deserialization failed.
    #[error("Failed to serialize or deserialize JSON")]
    Json(#[from] serde_json::Error),

    /// `MessagePack` encoding of an on-disk fragment payload failed.
    #[error("Failed to encode fragment to MessagePack")]
    MsgPackEncode(#[from] encode::Error),

    /// `MessagePack` decoding of an on-disk fragment payload failed.
    #[error("Failed to decode fragment from MessagePack")]
    MsgPackDecode(#[from] decode::Error),
}

/// The result type used throughout entity store operations.
pub type Result<T> = result::Result<T, EntityStoreError>;

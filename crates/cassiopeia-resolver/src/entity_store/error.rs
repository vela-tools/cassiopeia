use crate::{mapping_id::MappingId, store_action::StoreAction};
use rmp_serde::{decode, encode};
use std::{io, path::PathBuf, result};
use thiserror::Error;
use urn_rs::{Error as UrnError, Urn};

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

    /// Two fragments of one entity were written at the same place in the source, which a position
    /// stands for only one of.
    #[error("Two fragments of {entity} were written at the same place in the source")]
    RepeatedPosition {
        /// The entity both fragments contribute to.
        entity: Urn,
    },

    /// A key or value read back from a disk-backed store is too short to hold the source position it
    /// carries.
    #[error("A stored key or value of {length} bytes is too short to hold a source position")]
    TruncatedPosition {
        /// The length of the stored key or value, in bytes.
        length: usize,
    },

    /// An entity id read back from the disk-backed store is not a URN.
    #[error("The entity id '{id}' read back from the entity store is not a URN")]
    StoredId {
        /// The text read back.
        id: String,
        /// Why it is not a URN.
        #[source]
        source: UrnError,
    },

    /// A merge of an entity's records names a mapping no read set was registered for.
    #[error("The records of {entity} came from mapping {}, which no read set was registered for", mapping_id.as_u32())]
    UnregisteredMapping {
        /// The entity whose records were being merged.
        entity: Urn,
        /// The mapping the records came from.
        mapping_id: MappingId,
    },
}

/// The result type used throughout entity store operations.
pub type Result<T> = result::Result<T, EntityStoreError>;

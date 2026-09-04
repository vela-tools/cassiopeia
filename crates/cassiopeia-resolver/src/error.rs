use crate::{entity_store::error::EntityStoreError, relationship_store::error::RelationshipStoreError, store_batch_failure::StoreBatchFailure};
use cassiopeia_ngsi_ld::entity::error::NgsiLdError;
use std::{result, sync::Arc};
use thiserror::Error;
use urn_rs::Urn;

/// Errors raised during fragment resolution or entity assembly.
#[derive(Debug, Error)]
pub enum ResolverError {
    /// No mapping configuration was recorded for an assembled entity URN.
    ///
    /// The entity was stored but its mapping id could not be resolved during assembly, which should
    /// not happen in normal operation.
    #[error("Missing mapping configuration for entity '{entity}'")]
    MissingConfigForEntity {
        /// The URN whose mapping could not be found.
        entity: Urn,
    },

    /// An entity store operation failed.
    #[error("Entity store error")]
    EntityStore(#[from] EntityStoreError),

    /// A relationship store operation failed.
    #[error("Relationship store error")]
    RelationshipStore(#[from] RelationshipStoreError),

    /// A coalesced store batch failed.
    ///
    /// A disk-backed batch is all-or-nothing, so the same failure is reported for every fragment
    /// that participated in it. The store errors underneath are not `Clone`, so the failure is
    /// shared behind an [`Arc`] rather than rendered to text: sharing keeps the typed chain intact
    /// all the way down to the `redb` error, which a rendered message would have cut off.
    #[error("A coalesced store batch failed")]
    BatchFailure {
        /// The shared underlying failure.
        #[source]
        source: Arc<StoreBatchFailure>,
    },

    /// A stored relationship key was not a legal NGSI-LD attribute name.
    #[error("Invalid relationship attribute name")]
    RelationshipName(#[from] NgsiLdError),
}

/// The result type used throughout resolution.
pub type Result<T> = result::Result<T, ResolverError>;

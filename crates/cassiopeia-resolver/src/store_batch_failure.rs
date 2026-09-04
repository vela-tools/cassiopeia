use crate::{entity_store::error::EntityStoreError, relationship_store::error::RelationshipStoreError};
use thiserror::Error;

/// Why a coalesced store batch failed.
///
/// A batch writes relationship edges and entity fragments in that order, and either write can fail;
/// keeping both typed is what lets the run's report walk down to the `redb` error underneath instead
/// of stopping at a sentence someone assembled with `format!`.
#[derive(Debug, Error)]
pub enum StoreBatchFailure {
    /// The relationship edges could not be written.
    #[error("the relationship edges could not be written")]
    Relationship(#[source] RelationshipStoreError),

    /// The entity fragments could not be written.
    #[error("the entity fragments could not be written")]
    Entity(#[source] EntityStoreError),

    /// Both writes failed; the relationship failure is kept as the chained cause because it happened
    /// first.
    #[error("neither the relationship edges nor the entity fragments could be written: {entity}")]
    Both {
        /// The relationship write's failure.
        #[source]
        relationship: RelationshipStoreError,
        /// The entity write's failure.
        entity: EntityStoreError,
    },
}

impl StoreBatchFailure {
    /// Combines the two writes' outcomes into one failure, or `None` when the batch succeeded.
    #[must_use]
    pub fn of(relationship: Option<RelationshipStoreError>, entity: Option<EntityStoreError>) -> Option<StoreBatchFailure> {
        match (relationship, entity) {
            (Some(relationship), Some(entity)) => Some(StoreBatchFailure::Both { relationship, entity }),
            (Some(relationship), None) => Some(StoreBatchFailure::Relationship(relationship)),
            (None, Some(entity)) => Some(StoreBatchFailure::Entity(entity)),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_store::error::EntityStoreError, relationship_store::error::RelationshipStoreError, store_batch_failure::StoreBatchFailure};
    use std::{error::Error, io};

    fn relationship() -> RelationshipStoreError {
        RelationshipStoreError::CreateTempDirectory {
            source: io::Error::other("no space left on device"),
        }
    }

    fn entity() -> EntityStoreError {
        EntityStoreError::CreateTempDirectory {
            source: io::Error::other("permission denied"),
        }
    }

    #[test]
    fn a_batch_that_wrote_cleanly_has_no_failure() {
        assert!(StoreBatchFailure::of(None, None).is_none());
    }

    #[test]
    fn one_failed_write_keeps_its_own_error_as_the_cause() {
        let failure = StoreBatchFailure::of(None, Some(entity())).expect("a failure");

        assert!(failure.source().expect("a cause").to_string().contains("entity store"));
    }

    #[test]
    fn two_failed_writes_keep_both() {
        let failure = StoreBatchFailure::of(Some(relationship()), Some(entity())).expect("a failure");

        assert!(matches!(failure, StoreBatchFailure::Both { .. }));
        assert!(failure.to_string().contains("entity store"));
    }
}

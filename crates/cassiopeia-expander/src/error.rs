use crate::urn::error::UrnError;
use cassiopeia_common::collection::CollectionName;
use thiserror::Error;

/// Errors raised while expanding a record into fragments.
#[derive(Debug, Error)]
pub enum ExpanderError {
    /// URN or scope generation failed for an entity or one of its relationships.
    #[error(transparent)]
    Urn(#[from] UrnError),

    /// A record's collection label matched no mapping in a `Collections` binding.
    #[error("No mapping is bound to the source collection '{0}'")]
    UnmatchedCollection(CollectionName),

    /// A `Collections` binding requires a collection label, but the record carried none.
    #[error("A record without a source collection cannot be routed under a 'mappings' binding")]
    CollectionMissing,
}

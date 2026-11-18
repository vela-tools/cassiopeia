use crate::urn::{error::UrnError, scope_error::ScopeError};
use cassiopeia_common::collection::CollectionName;
use cassiopeia_mapping::template::error::TemplateError;
use thiserror::Error;

/// Why an entity a record describes is not written, or why the record describes none.
#[derive(Debug, Error)]
pub enum ExpanderError {
    /// Identifier generation failed for the entity or one of its relationships.
    #[error(transparent)]
    Urn(#[from] UrnError),

    /// The entity's scope could not be generated.
    #[error(transparent)]
    Scope(#[from] ScopeError),

    /// The entity's `expiresAt` template failed to resolve against the record. The template's own
    /// error names its declaration and mapping document, so it is the whole message.
    #[error(transparent)]
    Expiry(#[from] TemplateError),

    /// A record's collection label matched no mapping in a `Collections` binding.
    #[error("No mapping is bound to the source collection '{0}'")]
    UnmatchedCollection(CollectionName),

    /// A `Collections` binding requires a collection label, but the record carried none.
    #[error("A record without a source collection cannot be routed under a 'mappings' binding")]
    CollectionMissing,
}

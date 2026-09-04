use cassiopeia_mapping::template::error::TemplateError;
use cassiopeia_ngsi_ld::entity::{error::NgsiLdError, name::NameBuf};
use std::result;
use thiserror::Error;
use urn_rs::Error as UrnRsError;

/// Failures raised while minting NGSI-LD URNs and scopes for expanded entities.
#[derive(Debug, Error)]
pub enum UrnError {
    /// The `urn-rs` builder rejected the assembled namespace-specific string.
    ///
    /// The namespace identifier is not carried: Cassiopeia mints only `urn:ngsi-ld:` identifiers, so
    /// it is a constant and belongs in the sentence rather than in the value.
    #[error("Cannot mint a urn:ngsi-ld: identifier for the namespace-specific string '{nss}'")]
    BuildUrn {
        /// The namespace-specific string that was rejected.
        nss: Box<str>,
        /// The underlying builder error.
        #[source]
        source: UrnRsError,
    },

    /// A template failed to resolve against the source record.
    #[error("Template resolution failed")]
    Template(#[from] TemplateError),

    /// A resolved scope value is not a valid NGSI-LD scope.
    #[error("The resolved scope '{rejected}' is not a legal NGSI-LD scope")]
    InvalidScope {
        /// The rejected scope text, kept as-is because it is unvalidated input.
        rejected: Box<str>,
        /// Why the scope was rejected.
        #[source]
        source: NgsiLdError,
    },

    /// A resolved identifier is empty, so no URN can be built.
    #[error("The identifier resolved for a '{target_entity_type}' entity is empty, so no URN can be minted")]
    GeneratedIdEmpty {
        /// The entity type the empty identifier was meant to name.
        target_entity_type: NameBuf,
    },

    /// A relationship attribute has neither a compiled nor a raw source to derive an ID from.
    #[error("Relationship attribute is missing a source")]
    RelationshipMissingSource,

    /// A relationship attribute does not declare the entity it points at.
    #[error("No relationship target configured for attribute")]
    NoRelationshipTarget,
}

/// The result type used throughout URN generation.
pub type Result<T> = result::Result<T, UrnError>;

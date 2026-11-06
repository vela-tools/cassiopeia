use crate::urn::non_text_shape::NonTextShape;
use cassiopeia_mapping::template::{TemplateSource, error::TemplateError, field_path::FieldPath};
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

    /// A template failed to resolve against the source record. The template's own error names its
    /// declaration and mapping document, so it is the whole message.
    #[error(transparent)]
    Template(#[from] TemplateError),

    /// A resolved scope value cannot be the scope of the entity being written.
    #[error("The resolved scope '{rejected}' cannot be the entity's scope")]
    InvalidScope {
        /// The rejected scope text, kept as-is because it is unvalidated input.
        rejected: Box<str>,
        /// Why the scope was rejected.
        #[source]
        source: NgsiLdError,
    },

    /// A scope template reads a field whose value is not text where a scope needs text: a field read
    /// as the whole scope holds a boolean, a number or an object, or a field printed into a longer
    /// scope holds a boolean, an array or an object. Its JSON rendering, such as `true`, would be
    /// written as a scope the source never meant.
    #[error("The scope template `{template}` reads `{field}`, which holds {shape}, not a scope")]
    ScopeFieldNotText {
        /// The scope template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The field the template reads.
        field: FieldPath,
        /// What the field holds instead of text.
        shape: NonTextShape,
    },

    /// A scope template's expression resolves to a value that is not text, so its JSON rendering
    /// would be written as a scope the source never meant.
    #[error("The scope template `{template}` resolves to {shape}, not a scope")]
    ScopeValueNotText {
        /// The scope template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// What the expression resolves to instead of text.
        shape: NonTextShape,
    },

    /// A scope template resolves to a list of scopes, one of whose items is not text, so it names no
    /// scope of its own.
    #[error("The scope template `{template}` gives a list whose item {position} holds {shape}, not a scope")]
    ScopeItemNotText {
        /// The scope template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// Where the item sits in the list, counted from 1.
        position: usize,
        /// What the item holds instead of text.
        shape: NonTextShape,
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

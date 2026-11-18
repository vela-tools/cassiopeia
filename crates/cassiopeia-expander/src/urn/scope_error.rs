use crate::urn::non_text_shape::NonTextShape;
use cassiopeia_mapping::template::{TemplateSource, error::TemplateError, field_path::FieldPath};
use cassiopeia_ngsi_ld::entity::error::NgsiLdError;
use thiserror::Error;

/// Failures raised while generating the scope of an expanded entity.
#[derive(Debug, Error)]
pub enum ScopeError {
    /// A scope template failed to resolve against the source record. The template's own error names
    /// its declaration and mapping document, so it is the whole message.
    #[error(transparent)]
    Template(#[from] TemplateError),

    /// A resolved scope value cannot be the scope of the entity being written.
    #[error("The resolved scope '{rejected}' cannot be the entity's scope")]
    Invalid {
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
    FieldNotText {
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
    ValueNotText {
        /// The scope template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// What the expression resolves to instead of text.
        shape: NonTextShape,
    },

    /// A scope template resolves to a list of scopes, one of whose items is not text, so it names no
    /// scope of its own.
    #[error("The scope template `{template}` gives a list whose item {position} holds {shape}, not a scope")]
    ItemNotText {
        /// The scope template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// Where the item sits in the list, counted from 1.
        position: usize,
        /// What the item holds instead of text.
        shape: NonTextShape,
    },
}

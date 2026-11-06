use crate::context_load_error::ContextLoadError;
use cassiopeia_ngsi_ld::entity::value_type::ValueType;
use jsonld::ExpandError;
use thiserror::Error;

/// Why the `valueType` of an attribute could not be expanded to a datatype IRI.
#[derive(Debug, Error)]
pub enum ExpansionError {
    /// The bundled core `@context` is not JSON.
    #[error("The bundled NGSI-LD core @context cannot be read")]
    CoreContext(#[source] serde_json::Error),

    /// The HTTP client remote `@context` documents are fetched through could not be built.
    #[error("The HTTP client for loading @context documents cannot be built")]
    Client(#[source] ContextLoadError),

    /// The `@context` could not be turned into JSON to expand against.
    #[error("The @context cannot be written as JSON")]
    Context(#[source] serde_json::Error),

    /// The `@context` could not be loaded or processed, so nothing in it can be expanded.
    #[error("The @context cannot be loaded or processed")]
    Expansion(#[source] ExpandError<ContextLoadError>),

    /// A `valueType` expands to no absolute IRI under the `@context`: a relative reference with no
    /// term or vocabulary mapping to resolve it, or a blank node.
    #[error("'{value_type}' does not expand to an absolute IRI under the @context")]
    NotAnIri {
        /// The value type that did not expand.
        value_type: ValueType,
    },
}

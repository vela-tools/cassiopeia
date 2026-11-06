use iri_rs::IriRefBuf;
use jsonld_syntax::context::InvalidContext;
use thiserror::Error;

/// Why a JSON-LD context document could not be read into an [`NgsiLdContext`].
///
/// [`NgsiLdContext`]: crate::entity::context::NgsiLdContext
#[derive(Debug, Error)]
pub enum ContextDocumentError {
    /// The document is not well-formed JSON.
    #[error("The document is not valid JSON")]
    Unparsable(#[from] serde_json::Error),

    /// The document is not a JSON object with an `@context` member, which every context document
    /// must be (JSON-LD 1.1 Processing Algorithms and API, clause 4.1.2, step 5.2.5).
    #[error("The document has no top-level '@context' member")]
    ContextMemberMissing,

    /// The `@context` member does not follow the JSON-LD 1.1 context grammar (JSON-LD 1.1, clause
    /// 9.15).
    #[error("The '@context' member is not a valid JSON-LD 1.1 context")]
    Invalid(#[from] InvalidContext),

    /// A remote context reference is relative. The context is carried verbatim on every entity,
    /// and a broker has no base to resolve a relative reference against.
    #[error("'{reference}' is not an absolute URL; a remote context an entity carries must be one")]
    RemoteNotAbsolute {
        /// The reference that was rejected.
        reference: IriRefBuf,
        /// Why the reference is not an absolute URL.
        #[source]
        source: url::ParseError,
    },
}

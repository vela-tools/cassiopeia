use crate::{dereference::document_ref::DocumentRef, error::Result};
use serde_json::Value;
use std::sync::Arc;

/// Reads the document a `$ref` names when that document is not the one being expanded.
///
/// Kept as a trait so the expansion does not have to know whether the other document comes from the
/// catalog, from a test fixture, or from anywhere else.
pub trait SchemaResolver: Send + Sync {
    /// Reads the document `reference` names.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the reference names no stored document or
    /// the document cannot be read.
    fn resolve(&self, reference: DocumentRef<'_>) -> Result<Arc<Value>>;
}

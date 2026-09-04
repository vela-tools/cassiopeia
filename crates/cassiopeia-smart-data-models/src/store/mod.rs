pub mod catalog_index;
pub mod file_system;

use crate::{error::Result, schema_id::SchemaId};
use std::collections::BTreeMap;
use url::Url;

/// Reads and writes the schema documents themselves.
pub trait SchemaStore: Send + Sync {
    /// Reads a schema, or `None` when the store does not hold it.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when a stored schema cannot be read.
    fn get_schema(&self, id: &SchemaId) -> Result<Option<String>>;

    /// Writes a schema, replacing any earlier copy.
    ///
    /// The catalog index is not touched: a download writes thousands of schemas and rewriting the
    /// index once per schema would dominate its cost.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the schema cannot be written.
    fn put_schema(&self, id: &SchemaId, content: &str) -> Result<()>;
}

/// Reads and writes the index of what the catalog holds.
///
/// Separate from `SchemaStore` because it changes for different reasons: the index is rewritten
/// once at the end of a download, while schemas are written throughout it.
pub trait ModelCatalog: Send + Sync {
    /// Every schema the catalog holds.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the index cannot be read.
    fn list_models(&self) -> Result<Vec<SchemaId>>;

    /// Sets the index's model list to exactly `models`, replacing any earlier list.
    ///
    /// Recording is authoritative rather than additive so a re-download purges entries the catalog
    /// no longer lists as models, such as support schemas from an earlier download.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the index cannot be read or written.
    fn record_models(&self, models: &[SchemaId]) -> Result<()>;

    /// The `@context` document published for a schema, when one is known.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the index cannot be read.
    fn get_context_url(&self, id: &SchemaId) -> Result<Option<Url>>;

    /// Every known `@context` document.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the index cannot be read.
    fn get_all_context_urls(&self) -> Result<BTreeMap<SchemaId, Url>>;

    /// Adds `@context` documents to the index, replacing any earlier entry for the same schema.
    ///
    /// # Errors
    /// Returns an [`SdmError`](crate::error::SdmError) when the index cannot be read or written.
    fn record_context_urls(&self, contexts: &BTreeMap<SchemaId, Url>) -> Result<()>;
}

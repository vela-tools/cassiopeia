use crate::{
    dereference::{CircularReferences, ExternalReferences, dereference, document_ref::DocumentRef, resolver::SchemaResolver},
    error::{Result, SdmError},
    schema_id::SchemaId,
    store::{ModelCatalog, SchemaStore},
};
use dashmap::DashMap;
use fst::{IntoStreamer, Set, automaton::Subsequence};
use serde_json::Value;
use std::{str::FromStr, sync::Arc};
use url::Url;

/// The extension a `$ref` names a stored schema with.
const SCHEMA_SUFFIX: &str = ".json";

/// Whether parsed schemas are kept in memory after they are first read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caching {
    /// Keep every schema that has been read, which is what a pipeline validating many records
    /// wants.
    Enabled,

    /// Read from the store every time, which keeps memory flat for a one-shot lookup.
    Disabled,
}

/// Reads the Smart Data Models catalog: what it holds, and the schema behind each entry.
pub struct Catalog<S> {
    /// The store schemas and the index are read from.
    store: S,
    /// Parsed schemas kept in memory when caching is enabled, `None` otherwise.
    schemas: Option<DashMap<SchemaId, Arc<Value>>>,
}

impl<S> Catalog<S>
where
    S: SchemaStore + ModelCatalog,
{
    /// Opens the catalog over `store`.
    #[must_use]
    pub fn new(store: S, caching: Caching) -> Catalog<S> {
        let schemas = match caching {
            Caching::Enabled => Some(DashMap::new()),
            Caching::Disabled => None,
        };

        Catalog { store, schemas }
    }

    /// The store the catalog reads from.
    pub const fn store(&self) -> &S {
        &self.store
    }

    /// Every schema the catalog holds.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the catalog index cannot be read.
    pub fn list(&self) -> Result<Vec<SchemaId>> {
        self.store.list_models()
    }

    /// Every schema whose name contains `query`'s characters in order.
    ///
    /// Matching is against the schema's own name rather than its qualified form, because that is
    /// what somebody typing a model name knows.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the catalog index cannot be read or the search index cannot be
    /// built.
    pub fn search(&self, query: &str) -> Result<Vec<SchemaId>> {
        let models = self.list()?;

        let mut names: Vec<String> = models.iter().map(|id| id.name().as_str().to_lowercase()).collect();
        names.sort();
        names.dedup();

        let index = Set::from_iter(names).map_err(|source| SdmError::SearchIndex { source })?;
        let matches = index
            .search(Subsequence::new(&query.to_lowercase()))
            .into_stream()
            .into_strs()
            .map_err(|source| SdmError::SearchIndex { source })?;

        Ok(models
            .into_iter()
            .filter(|id| matches.iter().any(|matched| id.name().as_str().to_lowercase() == *matched))
            .collect())
    }

    /// The schema stored for `id`, exactly as it was written.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the stored schema cannot be read.
    pub fn get_schema_source(&self, id: &SchemaId) -> Result<Option<String>> {
        self.store.get_schema(id)
    }

    /// The schema stored for `id`, parsed.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the stored schema cannot be read or is not valid JSON.
    pub fn get_schema(&self, id: &SchemaId) -> Result<Option<Arc<Value>>> {
        if let Some(cached) = self.schemas.as_ref().and_then(|schemas| schemas.get(id)) {
            return Ok(Some(Arc::clone(cached.value())));
        }

        let Some(source) = self.store.get_schema(id)? else {
            return Ok(None);
        };

        let schema: Value = serde_json::from_str(&source).map_err(|source| SdmError::Json {
            path: id.to_string().into(),
            source,
        })?;
        let schema = Arc::new(schema);

        if let Some(schemas) = self.schemas.as_ref() {
            schemas.insert(id.clone(), Arc::clone(&schema));
        }

        Ok(Some(schema))
    }

    /// The schema stored for `id` with every `$ref` expanded, including those naming other stored
    /// schemas.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the schema cannot be read or a reference cannot be resolved.
    pub fn get_dereferenced_schema(&self, id: &SchemaId) -> Result<Option<Value>> {
        let Some(schema) = self.get_schema(id)? else {
            return Ok(None);
        };

        dereference(&schema, &ExternalReferences::Follow(self), CircularReferences::Break).map(Some)
    }

    /// The `@context` document published for `id`, when the catalog knows of one.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the catalog index cannot be read.
    pub fn get_context_url(&self, id: &SchemaId) -> Result<Option<Url>> {
        self.store.get_context_url(id)
    }
}

impl<S> SchemaResolver for Catalog<S>
where
    S: SchemaStore + ModelCatalog,
{
    fn resolve(&self, reference: DocumentRef<'_>) -> Result<Arc<Value>> {
        // A download rewrites every `$ref` that pointed at a published URL into the stored
        // schema's own path, so what arrives here is `dataModel.OCF/Sensor.json` rather than a URL.
        let uri = reference.as_str();
        let id = SchemaId::from_str(uri.strip_suffix(SCHEMA_SUFFIX).unwrap_or(uri))?;

        self.get_schema(&id)?.ok_or(SdmError::SchemaNotFound { id })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        catalog::{Caching, Catalog},
        schema_id::SchemaId,
        store::{ModelCatalog, SchemaStore, file_system::FileSystemStore},
    };
    use std::str::FromStr;
    use tempfile::TempDir;

    fn id(value: &str) -> SchemaId {
        SchemaId::from_str(value).expect("the identifier is well formed")
    }

    fn catalog() -> (TempDir, Catalog<FileSystemStore>) {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let store = FileSystemStore::new(directory.path()).expect("the store can be opened");

        store.put_schema(&id("Point"), r#"{"type": "array"}"#).expect("the schema can be written");
        store
            .put_schema(&id("dataModel.OCF/Sensor"), r#"{"properties": {"location": {"$ref": "Point.json"}}}"#)
            .expect("the schema can be written");
        store
            .record_models(&[id("Point"), id("dataModel.OCF/Sensor")])
            .expect("the index can be written");

        (directory, Catalog::new(store, Caching::Enabled))
    }

    #[test]
    fn a_stored_schema_is_read_back_parsed() {
        let (_directory, catalog) = catalog();

        let schema = catalog.get_schema(&id("Point")).unwrap().expect("the schema is stored");

        assert_eq!(schema["type"], "array");
    }

    #[test]
    fn a_schema_the_catalog_does_not_hold_reads_as_absent() {
        let (_directory, catalog) = catalog();

        assert!(catalog.get_schema(&id("Absent")).unwrap().is_none());
    }

    #[test]
    fn dereferencing_follows_a_reference_into_another_stored_schema() {
        let (_directory, catalog) = catalog();

        let schema = catalog
            .get_dereferenced_schema(&id("dataModel.OCF/Sensor"))
            .unwrap()
            .expect("the schema is stored");

        assert_eq!(schema["properties"]["location"]["type"], "array");
    }

    #[test]
    fn searching_matches_the_schema_name_rather_than_its_repository() {
        let (_directory, catalog) = catalog();

        assert_eq!(catalog.search("sensor").unwrap(), vec![id("dataModel.OCF/Sensor")]);
    }

    #[test]
    fn searching_for_something_absent_matches_nothing() {
        let (_directory, catalog) = catalog();

        assert!(catalog.search("weather").unwrap().is_empty());
    }
}

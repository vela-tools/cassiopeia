use crate::{
    error::{Result, SdmError},
    schema_id::SchemaId,
    store::{ModelCatalog, SchemaStore, catalog_index::CatalogIndex},
};
use cassiopeia_common::error::io::{IoAction, IoError};
use std::{
    collections::BTreeMap,
    fs::{create_dir_all, read_to_string, write},
    path::{Path, PathBuf},
};
use url::Url;

/// The file the catalog index is kept in, inside the store's directory.
const INDEX_FILE: &str = "manifest.json";
/// The extension every stored schema is written with.
const SCHEMA_EXTENSION: &str = "json";

/// A catalog kept as a directory of JSON files.
#[derive(Debug)]
pub struct FileSystemStore {
    directory: PathBuf,
    index_path: PathBuf,
}

impl FileSystemStore {
    /// Opens the catalog in `directory`, creating the directory and an empty index when they do
    /// not exist yet.
    ///
    /// # Errors
    /// Returns an [`SdmError`] when the directory or the initial empty index cannot be created.
    pub fn new(directory: impl Into<PathBuf>) -> Result<FileSystemStore> {
        let directory = directory.into();
        create_dir_all(&directory).map_err(|source| IoError::DirectoryOperation {
            source,
            path: directory.clone(),
            action: IoAction::Create,
        })?;

        let index_path = directory.join(INDEX_FILE);
        let store = FileSystemStore { directory, index_path };

        if !store.index_path.exists() {
            store.write_index(&CatalogIndex::default())?;
        }

        Ok(store)
    }

    /// Where the schema for `id` is kept.
    fn schema_path(&self, id: &SchemaId) -> PathBuf {
        let file = format!("{}.{SCHEMA_EXTENSION}", id.name());

        match id.repository() {
            Some(repository) => self.directory.join(repository.as_str()).join(file),
            None => self.directory.join(file),
        }
    }

    /// Reads the catalog index.
    fn read_index(&self) -> Result<CatalogIndex> {
        let content = read_file(&self.index_path)?;

        serde_json::from_str(&content).map_err(|source| SdmError::Json {
            path: self.index_path.clone(),
            source,
        })
    }

    /// Writes the catalog index.
    fn write_index(&self, index: &CatalogIndex) -> Result<()> {
        let content = serde_json::to_string_pretty(index).map_err(|source| SdmError::Json {
            path: self.index_path.clone(),
            source,
        })?;

        write_file(&self.index_path, &content)
    }
}

impl SchemaStore for FileSystemStore {
    fn get_schema(&self, id: &SchemaId) -> Result<Option<String>> {
        let path = self.schema_path(id);

        if path.exists() { read_file(&path).map(Some) } else { Ok(None) }
    }

    fn put_schema(&self, id: &SchemaId, content: &str) -> Result<()> {
        let path = self.schema_path(id);

        if let Some(parent) = path.parent() {
            create_dir_all(parent).map_err(|source| IoError::DirectoryOperation {
                source,
                path: parent.to_path_buf(),
                action: IoAction::Create,
            })?;
        }

        write_file(&path, content)
    }
}

impl ModelCatalog for FileSystemStore {
    fn list_models(&self) -> Result<Vec<SchemaId>> {
        Ok(self.read_index()?.models)
    }

    fn record_models(&self, models: &[SchemaId]) -> Result<()> {
        let mut index = self.read_index()?;
        index.set_models(models.iter().cloned());

        self.write_index(&index)
    }

    fn get_context_url(&self, id: &SchemaId) -> Result<Option<Url>> {
        Ok(self.read_index()?.contexts.remove(id))
    }

    fn get_all_context_urls(&self) -> Result<BTreeMap<SchemaId, Url>> {
        Ok(self.read_index()?.contexts)
    }

    fn record_context_urls(&self, contexts: &BTreeMap<SchemaId, Url>) -> Result<()> {
        let mut index = self.read_index()?;
        index.contexts.extend(contexts.iter().map(|(id, url)| (id.clone(), url.clone())));

        self.write_index(&index)
    }
}

/// Reads a file, naming it in any failure.
fn read_file(path: &Path) -> Result<String> {
    read_to_string(path).map_err(|source| {
        IoError::FileOperation {
            source,
            path: path.to_path_buf(),
            action: IoAction::Read,
        }
        .into()
    })
}

/// Writes a file, naming it in any failure.
fn write_file(path: &Path, content: &str) -> Result<()> {
    write(path, content).map_err(|source| {
        IoError::FileOperation {
            source,
            path: path.to_path_buf(),
            action: IoAction::Write,
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        schema_id::SchemaId,
        store::{ModelCatalog, SchemaStore, file_system::FileSystemStore},
    };
    use std::{collections::BTreeMap, str::FromStr};
    use tempfile::TempDir;
    use url::Url;

    fn store() -> (TempDir, FileSystemStore) {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let store = FileSystemStore::new(directory.path()).expect("the store can be opened");

        (directory, store)
    }

    fn id(value: &str) -> SchemaId {
        SchemaId::from_str(value).expect("the identifier is well formed")
    }

    #[test]
    fn a_schema_reads_back_exactly_as_it_was_written() {
        let (_directory, store) = store();

        store.put_schema(&id("dataModel.OCF/Sensor"), r#"{"type": "object"}"#).unwrap();

        assert_eq!(
            store.get_schema(&id("dataModel.OCF/Sensor")).unwrap(),
            Some(r#"{"type": "object"}"#.to_string())
        );
    }

    #[test]
    fn a_schema_that_was_never_written_reads_as_absent() {
        let (_directory, store) = store();

        assert_eq!(store.get_schema(&id("dataModel.OCF/Sensor")).unwrap(), None);
    }

    #[test]
    fn a_repository_qualified_schema_is_kept_under_its_repository() {
        let (directory, store) = store();

        store.put_schema(&id("dataModel.OCF/Sensor"), "{}").unwrap();

        assert!(directory.path().join("dataModel.OCF").join("Sensor.json").exists());
    }

    #[test]
    fn a_shared_schema_is_kept_beside_the_index() {
        let (directory, store) = store();

        store.put_schema(&id("common-schema"), "{}").unwrap();

        assert!(directory.path().join("common-schema.json").exists());
    }

    #[test]
    fn recording_models_lists_each_one_once() {
        let (_directory, store) = store();

        store
            .record_models(&[id("Point"), id("dataModel.OCF/Sensor"), id("dataModel.OCF/Sensor")])
            .unwrap();

        assert_eq!(store.list_models().unwrap(), vec![id("Point"), id("dataModel.OCF/Sensor")]);
    }

    #[test]
    fn recording_models_replaces_any_earlier_list() {
        let (_directory, store) = store();

        store.record_models(&[id("Point"), id("dataModel.OCF/Sensor")]).unwrap();
        store.record_models(&[id("dataModel.OCF/Sensor")]).unwrap();

        assert_eq!(store.list_models().unwrap(), vec![id("dataModel.OCF/Sensor")]);
    }

    #[test]
    fn a_context_url_reads_back_for_the_schema_it_was_recorded_for() {
        let (_directory, store) = store();
        let url = Url::parse("https://example.org/context.jsonld").unwrap();

        store.record_context_urls(&BTreeMap::from([(id("dataModel.OCF/Sensor"), url.clone())])).unwrap();

        assert_eq!(store.get_context_url(&id("dataModel.OCF/Sensor")).unwrap(), Some(url));
        assert_eq!(store.get_context_url(&id("Point")).unwrap(), None);
    }

    #[test]
    fn opening_an_existing_store_keeps_what_it_already_held() {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let first = FileSystemStore::new(directory.path()).unwrap();
        first.record_models(&[id("Point")]).unwrap();

        let second = FileSystemStore::new(directory.path()).unwrap();

        assert_eq!(second.list_models().unwrap(), vec![id("Point")]);
    }
}

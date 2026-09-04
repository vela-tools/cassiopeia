use cassiopeia_common::error::io::{IoAction, IoError};
use jsonschema::{Retrieve, Uri};
use serde_json::{Value, json};
use simd_json::{Error as SimdJsonError, serde::from_slice};
use std::{
    error::Error as StdError,
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Failures raised while resolving a local `$ref` to an on-disk schema file.
///
/// These are typed internally; the [`Retrieve`] trait fixes a boxed `dyn Error` return, so they are
/// boxed once at that single boundary rather than threaded through the crate untyped.
#[derive(Debug, Error)]
pub enum RetrieverError {
    /// No file backed the referenced local schema.
    #[error("Local schema file missing: {}", path.display())]
    SchemaFileMissing {
        /// The path that was looked up and not found.
        path: PathBuf,
    },

    /// The schema file could not be read from disk.
    #[error(transparent)]
    Io(#[from] IoError),

    /// The schema file could not be parsed as JSON.
    #[error("Failed to parse the local schema file at '{}'", path.display())]
    Parse {
        /// The parse failure reported by the JSON reader.
        #[source]
        source: SimdJsonError,
        /// The schema file that could not be parsed.
        path: PathBuf,
    },

    /// The schema file contained JSON `null`, which no validator can compile.
    #[error("Local schema file contains null: {}", path.display())]
    SchemaFileIsNull {
        /// The offending schema file.
        path: PathBuf,
    },
}

/// Resolves `$ref` URIs to local schema files during JSON Schema compilation.
///
/// References under `http://localhost/...` are resolved from local schema files. A reference to a
/// published host (`smart-data-models.github.io`, `raw.githubusercontent.com`, and the like) is also
/// resolved locally when the offline store holds the document its path names: downloaded Smart Data
/// Model schemas cross-reference one another by their absolute published URL, and the download-time
/// rewrite that would localise those URLs misses one whose casing differs from its rewrite entry. Any
/// other URI is answered with an empty schema `{}` to keep validation local and avoid network
/// requests; the referenced constraints are therefore not checked.
///
/// References resolve against an ordered list of root directories: each root is tried in turn, so a
/// custom schema's own directory can be searched first and the shared schemas folder used as a
/// fallback for common definitions.
#[derive(Clone, Debug)]
pub struct FileRetriever {
    /// The directories local references are resolved against, tried in order.
    roots: Vec<PathBuf>,
}

impl FileRetriever {
    /// Builds a retriever that resolves references against the given directories, in order.
    #[must_use]
    pub const fn rooted(roots: Vec<PathBuf>) -> FileRetriever {
        FileRetriever { roots }
    }

    /// Resolves the on-disk path a local reference points at.
    ///
    /// Each root is tried in order. When a schema with a qualified `$id` references a shared schema by
    /// a bare filename, the JSON Schema library resolves it relative to the `$id`, producing a nested
    /// path. Shared schemas live at the root of a folder, so within each root a nested path that does
    /// not exist falls back to the bare filename before the next root is tried.
    fn resolve_path(&self, relative: &str) -> Option<PathBuf> {
        for root in &self.roots {
            let direct = root.join(relative);
            if direct.exists() {
                return Some(direct);
            }

            if let Some(file_name) = Path::new(relative).file_name() {
                let fallback = root.join(file_name);
                if fallback.exists() {
                    return Some(fallback);
                }
            }
        }

        None
    }

    /// Reads, parses, and sanitizes the local schema a reference resolves to.
    ///
    /// This carries the crate's typed [`RetrieverError`]; [`Retrieve::retrieve`] boxes it once.
    fn retrieve_local(&self, relative: &str) -> Result<Value, RetrieverError> {
        let file_path = self.resolve_path(relative).ok_or_else(|| RetrieverError::SchemaFileMissing {
            path: self.roots.first().map_or_else(|| PathBuf::from(relative), |root| root.join(relative)),
        })?;

        let mut content = fs::read(&file_path).map_err(|source| IoError::FileOperation {
            source,
            path: file_path.clone(),
            action: IoAction::Read,
        })?;

        let mut schema: Value = from_slice(&mut content).map_err(|source| RetrieverError::Parse {
            source,
            path: file_path.clone(),
        })?;

        if schema.is_null() {
            return Err(RetrieverError::SchemaFileIsNull { path: file_path });
        }

        sanitize_schema(&mut schema);
        Ok(schema)
    }
}

impl Retrieve for FileRetriever {
    // The error type is fixed by the `Retrieve` trait, so this one boundary boxes the crate's typed
    // `RetrieverError` rather than carrying it untyped through the rest of the stage.
    fn retrieve(&self, uri: &Uri<String>) -> Result<Value, Box<dyn StdError + Send + Sync>> {
        let path = uri.path().as_str();
        let relative = path.strip_prefix('/').unwrap_or(path);
        let is_localhost = uri.authority().map(|authority| authority.as_str()) == Some("localhost");

        // A localhost reference is always a local schema and must resolve; a published-host reference
        // is resolved locally only when the store actually holds the document its path names, so an
        // unknown external reference still falls back to a permissive empty schema below.
        if is_localhost || self.resolve_path(relative).is_some() {
            return self
                .retrieve_local(relative)
                .map_err(|error| Box::new(error) as Box<dyn StdError + Send + Sync>);
        }

        Ok(json!({}))
    }
}

/// Iteratively repairs two invalid schema patterns found in real-world schemas.
///
/// `"items": null` becomes `"items": {}` (a null crashes validators), and a null `"required"` is
/// removed (`required` must be an array). An explicit stack avoids recursion so a deeply nested
/// schema cannot overflow the call stack.
pub fn sanitize_schema(root: &mut Value) {
    let mut stack = vec![root];

    while let Some(current) = stack.pop() {
        match current {
            Value::Object(map) => {
                if let Some(items) = map.get_mut("items")
                    && items.is_null()
                {
                    *items = json!({});
                }
                if let Some(required) = map.get("required")
                    && required.is_null()
                {
                    map.remove("required");
                }

                stack.extend(map.values_mut());
            }
            Value::Array(array) => stack.extend(array.iter_mut()),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::retriever::{FileRetriever, RetrieverError, sanitize_schema};
    use jsonschema::{Retrieve, Uri};
    use serde_json::{Value, json};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn a_reference_resolves_against_the_nested_path_first() {
        let directory = TempDir::new().unwrap();
        fs::create_dir_all(directory.path().join("nested")).unwrap();
        fs::write(directory.path().join("nested/schema.json"), r#"{"title": "nested"}"#).unwrap();
        let retriever = FileRetriever::rooted(vec![directory.path().to_path_buf()]);

        let schema = retriever.retrieve_local("nested/schema.json").unwrap();

        assert_eq!(schema["title"], Value::String("nested".to_string()));
    }

    #[test]
    fn a_nested_reference_that_is_missing_falls_back_to_the_bare_filename() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("schema.json"), r#"{"title": "root"}"#).unwrap();
        let retriever = FileRetriever::rooted(vec![directory.path().to_path_buf()]);

        let schema = retriever.retrieve_local("some/where/schema.json").unwrap();

        assert_eq!(schema["title"], Value::String("root".to_string()));
    }

    #[test]
    fn resolve_path_tries_roots_in_order() {
        // A shared definition lives only in the second root; the schema's own first root does not
        // hold it, so resolution must fall through to the second root rather than stop at the first.
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        fs::write(second.path().join("defs.json"), r#"{"title": "shared"}"#).unwrap();
        let retriever = FileRetriever::rooted(vec![first.path().to_path_buf(), second.path().to_path_buf()]);

        let schema = retriever.retrieve_local("defs.json").unwrap();

        assert_eq!(schema["title"], Value::String("shared".to_string()));
    }

    #[test]
    fn an_earlier_root_wins_over_a_later_one() {
        // Both roots hold the reference; the first root in the list is the one that answers.
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        fs::write(first.path().join("defs.json"), r#"{"title": "first"}"#).unwrap();
        fs::write(second.path().join("defs.json"), r#"{"title": "second"}"#).unwrap();
        let retriever = FileRetriever::rooted(vec![first.path().to_path_buf(), second.path().to_path_buf()]);

        let schema = retriever.retrieve_local("defs.json").unwrap();

        assert_eq!(schema["title"], Value::String("first".to_string()));
    }

    #[test]
    fn a_published_host_reference_resolves_locally_when_the_store_holds_it() {
        let directory = TempDir::new().unwrap();
        fs::create_dir_all(directory.path().join("dataModel.Weather")).unwrap();
        fs::write(directory.path().join("dataModel.Weather/weather-schema.json"), r#"{"title": "weather"}"#).unwrap();
        let retriever = FileRetriever::rooted(vec![directory.path().to_path_buf()]);
        let uri = Uri::parse("https://smart-data-models.github.io/dataModel.Weather/weather-schema.json".to_string()).unwrap();

        let schema = retriever.retrieve(&uri).unwrap();

        assert_eq!(schema["title"], Value::String("weather".to_string()));
    }

    #[test]
    fn an_unknown_external_reference_falls_back_to_an_empty_schema() {
        let directory = TempDir::new().unwrap();
        let retriever = FileRetriever::rooted(vec![directory.path().to_path_buf()]);
        let uri = Uri::parse("https://example.org/whatever.json".to_string()).unwrap();

        assert_eq!(retriever.retrieve(&uri).unwrap(), json!({}));
    }

    #[test]
    fn a_missing_reference_is_reported_as_missing() {
        let directory = TempDir::new().unwrap();
        let retriever = FileRetriever::rooted(vec![directory.path().to_path_buf()]);

        let error = retriever.retrieve_local("absent.json").unwrap_err();

        assert!(matches!(error, RetrieverError::SchemaFileMissing { .. }));
    }

    #[test]
    fn a_null_local_schema_is_reported_as_null() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("null.json"), "null").unwrap();
        let retriever = FileRetriever::rooted(vec![directory.path().to_path_buf()]);

        let error = retriever.retrieve_local("null.json").unwrap_err();

        assert!(matches!(error, RetrieverError::SchemaFileIsNull { .. }));
    }

    #[test]
    fn sanitize_repairs_null_items_and_removes_null_required() {
        let mut schema = json!({
            "type": "array",
            "items": null,
            "required": null,
            "nested": { "items": null }
        });

        sanitize_schema(&mut schema);

        assert_eq!(schema["items"], json!({}));
        assert!(schema.get("required").is_none());
        assert_eq!(schema["nested"]["items"], json!({}));
    }
}

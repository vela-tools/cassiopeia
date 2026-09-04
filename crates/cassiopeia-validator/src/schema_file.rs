use crate::{
    error::{Result, ValidatorError},
    retriever::sanitize_schema,
};
use cassiopeia_common::error::io::{IoAction, IoError};
use serde_json::Value;
use simd_json::serde::from_slice;
use std::{fs, path::Path};

/// Reads, parses, and sanitizes a schema file that is known to exist.
///
/// # Errors
/// Returns a [`ValidatorError`] when the file cannot be read, is not JSON, or is JSON `null`.
pub fn load_schema(path: &Path) -> Result<Value> {
    let mut content = fs::read(path).map_err(|source| IoError::FileOperation {
        source,
        path: path.to_path_buf(),
        action: IoAction::Read,
    })?;

    let mut schema: Value = from_slice(&mut content).map_err(|source| ValidatorError::ParseSchemaFile {
        source,
        path: path.to_path_buf(),
    })?;

    if schema.is_null() {
        return Err(ValidatorError::SchemaFileIsNull { path: path.to_path_buf() });
    }

    sanitize_schema(&mut schema);
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use crate::{error::ValidatorError, schema_file::load_schema};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn a_well_formed_schema_file_loads() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("Sensor.json");
        fs::write(&path, r#"{"type": "object"}"#).unwrap();

        assert!(load_schema(&path).unwrap().is_object());
    }

    #[test]
    fn a_null_schema_file_is_an_error() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("Sensor.json");
        fs::write(&path, "null").unwrap();

        assert!(matches!(load_schema(&path), Err(ValidatorError::SchemaFileIsNull { .. })));
    }

    #[test]
    fn an_unparseable_schema_file_is_an_error() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("Sensor.json");
        fs::write(&path, "{ not json").unwrap();

        assert!(matches!(load_schema(&path), Err(ValidatorError::ParseSchemaFile { .. })));
    }

    #[test]
    fn a_missing_schema_file_surfaces_the_io_failure() {
        let directory = TempDir::new().unwrap();

        assert!(matches!(load_schema(&directory.path().join("absent.json")), Err(ValidatorError::Io(_))));
    }
}

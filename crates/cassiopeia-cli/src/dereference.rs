use crate::error::{CliError, IoAction, Result};
use cassiopeia_smart_data_models::{
    catalog::{Caching, Catalog},
    dereference::{CircularReferences, ExternalReferences, dereference},
    store::file_system::FileSystemStore,
};
use serde_json::Value;
use std::{
    fs::{metadata, read_to_string},
    path::Path,
};

/// Prints a JSON Schema with every `$ref` expanded, resolving references to sibling schemas in the
/// same folder through a catalog rooted at that folder.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the schema is missing or cannot be
/// dereferenced.
pub fn dereference_schema(schema: &Path) -> Result<()> {
    if metadata(schema).is_err() {
        return Err(CliError::SchemaFileNotFound { path: schema.to_path_buf() });
    }

    let base_dir = schema.parent().unwrap_or_else(|| Path::new("."));

    let schema_content = read_to_string(schema).map_err(|source| CliError::FileOperation {
        source,
        path: schema.to_path_buf(),
        action: IoAction::Read,
    })?;

    let schema_val: Value = serde_json::from_str(&schema_content).map_err(|source| CliError::DeserializeSchema {
        source,
        path: schema.to_path_buf(),
    })?;

    let store = FileSystemStore::new(base_dir)?;
    let catalog = Catalog::new(store, Caching::Enabled);
    let dereferenced = dereference(&schema_val, &ExternalReferences::Follow(&catalog), CircularReferences::Break)?;

    let json = serde_json::to_string_pretty(&dereferenced).map_err(CliError::SerializeSchema)?;
    println!("{json}");

    Ok(())
}

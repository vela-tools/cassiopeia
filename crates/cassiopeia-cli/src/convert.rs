use crate::error::{CliError, IoAction, Result};
use serde_json::Value;
use std::{
    fs::{File, metadata, read_to_string},
    io::Write,
    path::{Path, PathBuf},
};

/// Converts a JSON5 mapping document to a plain-JSON one, refusing to overwrite an existing output,
/// and returns the path written.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the input is missing, the output already
/// exists, or the document cannot be parsed or written.
pub fn convert_mapping(mapping: &Path, output: &Path) -> Result<PathBuf> {
    if metadata(mapping).is_err() {
        return Err(CliError::MappingNotFound { path: mapping.to_path_buf() });
    }

    if metadata(output).is_ok() {
        return Err(CliError::FileAlreadyExists { path: output.to_path_buf() });
    }

    let content = read_to_string(mapping).map_err(|source| CliError::FileOperation {
        source,
        path: mapping.to_path_buf(),
        action: IoAction::Read,
    })?;

    let configuration: Value = serde_json5::from_str(&content).map_err(|source| CliError::DeserializeMappingJson5 {
        source,
        path: mapping.to_path_buf(),
    })?;

    let json = serde_json::to_string_pretty(&configuration).map_err(|source| CliError::SerializeMappingJson {
        source,
        path: output.to_path_buf(),
    })?;

    let mut file = File::create(output).map_err(|source| CliError::FileOperation {
        source,
        path: output.to_path_buf(),
        action: IoAction::Create,
    })?;

    file.write_all(json.as_bytes()).map_err(|source| CliError::FileOperation {
        source,
        path: output.to_path_buf(),
        action: IoAction::Write,
    })?;

    Ok(output.to_path_buf())
}

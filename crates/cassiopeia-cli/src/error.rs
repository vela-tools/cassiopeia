use cassiopeia_data_profiler::error::DataProfilerError;
use cassiopeia_ngsi_ld::data_model::DataModel;
use cassiopeia_smart_data_models::{error::SdmError, schema_id::SchemaIdError};
use cassiopeia_tui::error::TuiError;
use std::{io, path::PathBuf, result, str::Utf8Error};
use strum::Display;
use thiserror::Error;
use toml::ser::Error as TomlSerError;

/// Which file operation failed, named in a file-operation error message.
#[derive(Debug, Display)]
pub enum IoAction {
    Open,
    Create,
    Read,
    Write,
}

/// A failure raised by one of the CLI command handlers.
#[derive(Debug, Error)]
pub enum CliError {
    #[error("Failed to {action} file at '{}'", path.display())]
    FileOperation { source: io::Error, path: PathBuf, action: IoAction },

    #[error("File already exists: {}", path.display())]
    FileAlreadyExists { path: PathBuf },

    #[error("Wrong mapping file extension, expected .json5: {}", path.display())]
    WrongMappingFileExtension { path: PathBuf },

    #[error("Failed to parse document for formatting: {}", path.display())]
    ParseDocumentForFormatting { source: json5format::Error, path: PathBuf },

    #[error("Failed to format document")]
    FormatDocument(#[source] json5format::Error),

    #[error("Failed to convert document to UTF-8")]
    ConvertDocumentToUtf8(#[source] json5format::Error),

    #[error("Failed to parse document as UTF-8")]
    ParseDocumentAsUtf8(#[source] Utf8Error),

    #[error("Failed to get filename from document")]
    GetFilenameFromDocument,

    #[error("Failed to deserialize JSON5 mapping: {}", path.display())]
    DeserializeMappingJson5 { source: serde_json5::Error, path: PathBuf },

    #[error("Failed to serialize JSON mapping: {}", path.display())]
    SerializeMappingJson { source: serde_json::Error, path: PathBuf },

    #[error("Mapping not found at '{}'", path.display())]
    MappingNotFound { path: PathBuf },

    #[error("Schema file not found at '{}'", path.display())]
    SchemaFileNotFound { path: PathBuf },

    #[error("The catalog holds no schema for the data model '{model}'")]
    SchemaNotFound { model: DataModel },

    #[error("Failed to deserialize schema: {}", path.display())]
    DeserializeSchema { source: serde_json::Error, path: PathBuf },

    #[error("Failed to serialize schema")]
    SerializeSchema(#[source] serde_json::Error),

    #[error("Failed to serialize the default configuration")]
    SerializeConfig(#[source] TomlSerError),

    #[error("Failed to start the download runtime")]
    AsyncRuntime(#[source] io::Error),

    #[error("A Smart Data Models operation failed")]
    Sdm(#[from] SdmError),

    #[error("The schema identifier is not valid")]
    SchemaId(#[from] SchemaIdError),

    #[error("The terminal interface failed")]
    Tui(#[from] TuiError),

    #[error("The data profiler failed")]
    Profiler(#[from] DataProfilerError),
}

pub type Result<T, E = CliError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::error::IoAction;

    #[test]
    fn io_actions_render_as_their_verb() {
        assert_eq!(IoAction::Open.to_string(), "Open");
        assert_eq!(IoAction::Create.to_string(), "Create");
        assert_eq!(IoAction::Read.to_string(), "Read");
        assert_eq!(IoAction::Write.to_string(), "Write");
    }
}

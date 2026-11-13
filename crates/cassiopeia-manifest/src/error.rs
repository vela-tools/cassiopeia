use cassiopeia_common::{collection::CollectionName, error::io::IoError, format::DataFormat, masked_url::MaskedUrl};
use cassiopeia_sensorthings::{cursor_name::CursorName, service_root::ServiceRootError};
use std::{path::PathBuf, result, string::FromUtf8Error};
use thiserror::Error;

/// Failures raised while reading, validating, or writing a manifest document.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// A filesystem operation on the manifest failed.
    #[error(transparent)]
    Io(#[from] IoError),

    /// The manifest document could not be parsed as JSON5.
    #[error("Failed to parse the manifest at '{}'", path.display())]
    Parse {
        /// The path of the manifest that failed to parse.
        path: PathBuf,
        /// The underlying JSON5 parse error.
        source: serde_json5::Error,
    },

    /// The manifest declared no inputs, so it describes no work.
    #[error("A manifest must declare at least one input")]
    NoInputs,

    /// An input set both `mapping` and `mappings`, so its routing is ambiguous.
    #[error("An input must declare either 'mapping' or 'mappings', not both")]
    ConflictingMappingBinding,

    /// An input set neither `mapping` nor `mappings`, so no mapping governs its records.
    #[error("An input must declare a 'mapping' or a 'mappings' list")]
    MissingMappingBinding,

    /// An input's `mappings` list was empty, so it routes no collection.
    #[error("An input's 'mappings' list must not be empty")]
    EmptyCollections,

    /// An input's `mappings` list bound the same collection label twice.
    #[error("The collection '{0}' is bound more than once in one input's 'mappings'")]
    DuplicateCollection(CollectionName),

    /// An input bound collections under a format that cannot pack several of them.
    #[error("The format '{0}' does not support multiple collections, so 'mappings' is not allowed")]
    CollectionsUnsupportedByFormat(DataFormat),

    /// An input stated a CSV dialect without declaring the CSV format, so the dialect may describe
    /// nothing the source holds.
    #[error("An input's 'dialect' describes a CSV source, so the input must declare 'format: \"csv\"'")]
    DialectWithoutCsvFormat,

    /// An input declared request headers on a local file source, which is read without any request
    /// to send them on.
    #[error("The input '{}' is a local file, so it cannot declare 'headers'; headers are sent only when fetching a URL", path.display())]
    HeadersOnLocalSource {
        /// The local path the input reads.
        path: PathBuf,
    },

    /// An input stated `sensorthings` settings without declaring the SensorThings format, so the
    /// settings may describe nothing the source is.
    #[error("An input's 'sensorthings' settings describe a SensorThings server, so the input must declare 'format: \"sensorthings\"'")]
    SensorThingsSettingsWithoutFormat,

    /// A local SensorThings snapshot declared `sensorthings` settings, though a snapshot is read as it
    /// was collected.
    #[error("The input '{}' is a SensorThings snapshot file, which is read as it was collected, so it cannot declare 'sensorthings' settings", path.display())]
    SensorThingsSettingsOnSnapshot {
        /// The local path the input reads.
        path: PathBuf,
    },

    /// A remote SensorThings input's source is no service root, which every request is built on.
    #[error("The SensorThings input '{url}' is no service root")]
    NotAServiceRoot {
        /// The source, masked. Boxed so a URL does not enlarge every `Result` in the crate.
        url: Box<MaskedUrl>,
        /// Why it is no service root.
        source: ServiceRootError,
    },

    /// A remote SensorThings input's service root read a value from the environment.
    #[error(
        "The SensorThings service root '{url}' reads the environment; credentials for a SensorThings server go in 'headers', so every URL a read requests, logs and writes stays free of them"
    )]
    ServiceRootReadsEnvironment {
        /// The source, masked. Boxed so a URL does not enlarge every `Result` in the crate.
        url: Box<MaskedUrl>,
    },

    /// Two inputs read incrementally through one cursor, so each would move the positions the other
    /// resumes from.
    #[error("The cursor '{0}' is named by more than one input; each incremental input keeps its positions in a cursor of its own")]
    DuplicateCursor(CursorName),

    /// The output's destination settings could not be read: no known `target`, a missing setting,
    /// or a key neither the output nor its destination defines.
    #[error("The manifest's 'output' destination is invalid: {source}")]
    Destination {
        /// What serde objected to.
        source: serde_json::Error,
    },

    /// The manifest could not be serialized to JSON prior to formatting.
    #[error("Failed to serialize the manifest")]
    Serialize(#[from] serde_json::Error),

    /// The serialized manifest could not be rendered as formatted JSON5.
    ///
    /// `json5format::Error` does not implement `std::error::Error`, so it cannot be a `#[from]`
    /// source and is carried as its rendered message instead.
    #[error("Failed to format the manifest as JSON5: {message}")]
    Format {
        /// The rendered message from `json5format`.
        message: String,
    },

    /// The formatted JSON5 bytes were not valid UTF-8.
    #[error("The formatted manifest is not valid UTF-8")]
    Encoding(#[from] FromUtf8Error),
}

/// The result type every fallible manifest operation returns.
pub type Result<T, E = ManifestError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::error::ManifestError;
    use cassiopeia_common::error::io::{IoAction, IoError};
    use std::{io, path::PathBuf};

    #[test]
    fn an_io_failure_is_carried_transparently() {
        let error = ManifestError::from(IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/manifests/run.json5"),
            action: IoAction::Read,
        });

        assert!(error.to_string().contains("/manifests/run.json5"));
    }

    #[test]
    fn a_serialize_failure_chains_from_a_serde_json_error() {
        let serde_error = serde_json::from_str::<serde_json::Value>("{ not json").unwrap_err();
        let error = ManifestError::from(serde_error);

        assert!(matches!(error, ManifestError::Serialize(_)));
    }

    #[test]
    fn the_no_inputs_error_names_the_requirement() {
        assert!(ManifestError::NoInputs.to_string().contains("at least one input"));
    }
}

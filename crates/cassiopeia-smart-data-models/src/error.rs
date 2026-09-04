use crate::schema_id::{SchemaId, SchemaIdError};
use cassiopeia_common::error::io::IoError;
use std::{path::PathBuf, result};
use thiserror::Error;

/// Failures raised while working with the Smart Data Models catalog.
#[derive(Debug, Error)]
pub enum SdmError {
    /// A filesystem operation on the store failed.
    #[error(transparent)]
    Io(#[from] IoError),

    /// The HTTP client used to fetch the catalog could not be built.
    #[error("Failed to build the HTTP client for the Smart Data Models catalog")]
    BuildClient {
        /// The client-construction failure reported by the HTTP layer.
        source: reqwest::Error,
    },

    /// A request for a catalog document failed.
    #[error("The request to '{url}' failed")]
    Request {
        /// The document that was requested, as its URL text.
        url: String,
        /// The transport failure reported by the HTTP layer.
        source: reqwest::Error,
    },

    /// A document was served with a content type that is not a schema.
    #[error("'{url}' answered with content type '{content_type}', which is not a schema")]
    UnexpectedContentType {
        /// The document that was requested, as its URL text.
        url: String,
        /// The content type the server actually returned, verbatim.
        content_type: String,
    },

    /// A stored document could not be read as JSON.
    #[error("Failed to read '{}' as JSON", path.display())]
    Json {
        /// The document that could not be parsed.
        path: PathBuf,
        /// The parse failure reported by the JSON reader.
        source: serde_json::Error,
    },

    /// A downloaded document was not valid JSON.
    #[error("The schema downloaded from '{url}' is not valid JSON")]
    DownloadedJson {
        /// The document that was downloaded, as its URL text.
        url: String,
        /// The parse failure reported by the JSON reader.
        source: serde_json::Error,
    },

    /// A schema identifier was malformed.
    #[error(transparent)]
    SchemaId(#[from] SchemaIdError),

    /// The catalog search index could not be built.
    #[error("Failed to build the search index over the catalog")]
    SearchIndex {
        /// The index-construction failure reported by the FST layer.
        source: fst::Error,
    },

    /// No schema is stored for an identifier.
    #[error("No schema is stored for '{id}'")]
    SchemaNotFound {
        /// The identifier that resolved to nothing.
        id: SchemaId,
    },

    /// A `$ref` reached outside its schema while external resolution was disabled.
    #[error("'{reference}' points outside the schema, and external references are not being resolved")]
    ExternalReferenceRejected {
        /// The rejected reference, echoed back for diagnosis.
        reference: String,
    },

    /// A `$ref` closed a cycle while cycles were being refused.
    #[error("'{reference}' is part of a reference cycle")]
    CircularReference {
        /// The reference that closed the cycle.
        reference: String,
    },

    /// A `$ref` fragment was not a valid JSON pointer.
    #[error("'{pointer}' is not a JSON pointer")]
    InvalidJsonPointer {
        /// The rejected pointer, echoed back for diagnosis.
        pointer: String,
    },

    /// A JSON pointer named nothing in the schema.
    #[error("'{pointer}' does not point at anything in the schema")]
    UnresolvableJsonPointer {
        /// The pointer that resolved to nothing.
        pointer: String,
    },
}

/// The result type every fallible catalog operation returns.
pub type Result<T, E = SdmError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::error::SdmError;
    use cassiopeia_common::error::io::{IoAction, IoError};
    use std::{io, path::PathBuf};

    #[test]
    fn an_io_failure_is_wrapped_transparently() {
        let io = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/store/Sensor.json"),
            action: IoAction::Read,
        };

        assert!(SdmError::from(io).to_string().contains("Sensor.json"));
    }

    #[test]
    fn an_unexpected_content_type_names_the_url_and_the_type() {
        let error = SdmError::UnexpectedContentType {
            url: "https://example.org/schema.json".to_string(),
            content_type: "text/html".to_string(),
        };
        let message = error.to_string();

        assert!(message.contains("example.org"));
        assert!(message.contains("text/html"));
    }
}

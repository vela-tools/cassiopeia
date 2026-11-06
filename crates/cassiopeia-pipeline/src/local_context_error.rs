use cassiopeia_ngsi_ld::entity::context_document_error::ContextDocumentError;
use std::{io, path::PathBuf};
use thiserror::Error;

/// Why a local `@context` file could not be attached to a run's entities.
///
/// Each of these aborts the run before anything is written: entities written without the context
/// the run asked for expand against the core context's default vocabulary, so their attributes land
/// under different IRIs than the context defines.
#[derive(Debug, Error)]
pub enum LocalContextError {
    /// The file could not be read.
    #[error("Cannot read the local @context file '{}'", path.display())]
    Unreadable {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: io::Error,
    },

    /// The file was read but does not hold a usable JSON-LD context document.
    #[error("Cannot read the @context from '{}'", path.display())]
    Unusable {
        /// The file whose content was rejected.
        path: PathBuf,
        /// Why the content was rejected.
        #[source]
        source: ContextDocumentError,
    },
}

#[cfg(test)]
mod tests {
    use crate::local_context_error::LocalContextError;
    use cassiopeia_ngsi_ld::entity::context_document_error::ContextDocumentError;
    use std::{error::Error, path::PathBuf};

    #[test]
    fn an_unusable_file_names_its_path_and_chains_the_document_failure() {
        let error = LocalContextError::Unusable {
            path: PathBuf::from("/contexts/expanded.jsonld"),
            source: ContextDocumentError::ContextMemberMissing,
        };

        assert!(error.to_string().contains("/contexts/expanded.jsonld"));
        assert!(error.source().is_some_and(|source| source.to_string().contains("'@context'")));
    }
}

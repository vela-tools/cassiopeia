use std::{io, path::PathBuf};
use strum::Display;
use thiserror::Error;

/// The filesystem operation that was being attempted when an [`IoError`] arose.
///
/// Carried in the error so a message can name what failed rather than only where.
#[derive(Debug, Display)]
pub enum IoAction {
    /// Opening an existing path for reading.
    Open,
    /// Creating a new file.
    Create,
    /// Reading bytes from an open handle.
    Read,
    /// Reading a single entry from a directory listing.
    ReadEntry,
    /// Writing bytes to an open handle.
    Write,
    /// Flushing buffered writes to the underlying handle.
    Flush,
}

/// Failures raised while reading from or writing to the filesystem.
#[derive(Debug, Error)]
pub enum IoError {
    /// A file operation failed at a known path.
    #[error("Failed to {action} file at '{}'", path.display())]
    FileOperation {
        /// The underlying operating-system error.
        source: io::Error,
        /// The path the operation targeted.
        path: PathBuf,
        /// The operation that failed.
        action: IoAction,
    },

    /// A directory operation failed at a known path.
    #[error("Failed to {action} directory at '{}'", path.display())]
    DirectoryOperation {
        /// The underlying operating-system error.
        source: io::Error,
        /// The path the operation targeted.
        path: PathBuf,
        /// The operation that failed.
        action: IoAction,
    },

    /// A path expected to be a directory turned out to be a file.
    #[error("Expected a directory but found a file: {}", path.display())]
    NotADirectory {
        /// The offending path.
        path: PathBuf,
    },

    /// A file that must not already exist was found.
    #[error("File already exists: {}", path.display())]
    FileAlreadyExists {
        /// The offending path.
        path: PathBuf,
    },

    /// A required path was not present.
    #[error("Path does not exist: {}", path.display())]
    DoesNotExist {
        /// The missing path.
        path: PathBuf,
    },

    /// A path could not be interpreted as the structure the caller required.
    #[error("Invalid path structure: {}", path.display())]
    InvalidPathStructure {
        /// The path whose structure was rejected.
        path: PathBuf,
    },

    /// A temporary directory could not be created.
    #[error("Failed to create temporary directory")]
    CreateTempDirectory {
        /// The underlying operating-system error.
        source: io::Error,
    },

    /// A generic reader could not be drained.
    #[error("Failed to read from reader")]
    ReadFromReader {
        /// The underlying operating-system error.
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use crate::error::io::{IoAction, IoError};
    use std::{io, path::PathBuf};

    #[test]
    fn an_action_displays_as_its_variant_name() {
        assert_eq!(IoAction::Read.to_string(), "Read");
        assert_eq!(IoAction::ReadEntry.to_string(), "ReadEntry");
    }

    #[test]
    fn a_file_operation_error_names_the_path_and_the_action() {
        let error = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/data/stations.csv"),
            action: IoAction::Write,
        };
        let message = error.to_string();

        assert!(message.contains("/data/stations.csv"));
        assert!(message.contains("Write"));
    }
}

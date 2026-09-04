use crate::inspectors::csv::error::CsvInspectError;
use cassiopeia_common::error::io::IoError;
use std::{path::PathBuf, result};
use thiserror::Error;

/// Failures raised while profiling a file or byte buffer for its data format.
#[derive(Error, Debug)]
pub enum DataProfilerError {
    /// The file content could not be read from disk.
    #[error(transparent)]
    Io(#[from] IoError),

    /// A CSV dialect could not be inspected from a declared CSV payload.
    #[error(transparent)]
    CsvInspect(#[from] CsvInspectError),

    /// The requested path does not exist on disk.
    #[error("File does not exist: {0}")]
    NotFound(PathBuf),

    /// No detector recognised the content of the file at the given path.
    #[error("Could not determine data format for file: {0}")]
    UnknownFormat(PathBuf),

    /// No detector recognised the content of the provided byte buffer.
    #[error("Could not determine data format for provided bytes")]
    UnknownBytesFormat,
}

/// Convenience alias for results produced by the data profiler.
pub type Result<T> = result::Result<T, DataProfilerError>;

#[cfg(test)]
mod tests {
    use crate::error::DataProfilerError;
    use cassiopeia_common::error::io::{IoAction, IoError};
    use std::{io, path::PathBuf};

    #[test]
    fn an_io_failure_wraps_the_underlying_io_error_transparently() {
        let inner = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/data/stations.csv"),
            action: IoAction::Read,
        };
        let error = DataProfilerError::Io(inner);

        assert!(error.to_string().contains("/data/stations.csv"));
        assert!(error.to_string().contains("Read"));
    }

    #[test]
    fn a_not_found_error_names_the_missing_path() {
        let error = DataProfilerError::NotFound(PathBuf::from("/nope"));
        assert!(error.to_string().contains("/nope"));
    }
}

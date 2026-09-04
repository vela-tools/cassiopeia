use std::{io, path::PathBuf};

/// Errors that can occur while parsing CSV input.
#[derive(Debug, thiserror::Error)]
pub enum CsvIngestError {
    /// The CSV ingestor was handed an in-memory byte payload; it needs a file.
    #[error("the CSV ingestor requires a file, not bytes")]
    RequiresFile,

    /// The CSV file could not be opened for reading.
    #[error("could not open the CSV file at '{}'", path.display())]
    Open {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The file that could not be opened.
        path: PathBuf,
    },

    /// The underlying CSV reader failed to build or parse a record.
    #[error(transparent)]
    Reader(#[from] ::csv::Error),
}

#[cfg(test)]
mod tests {
    use crate::csv::error::CsvIngestError;
    use std::{error::Error, io, path::PathBuf};

    #[test]
    fn an_open_failure_names_the_file_and_keeps_the_operating_system_reason() {
        let error = CsvIngestError::Open {
            source: io::Error::other("permission denied"),
            path: PathBuf::from("/data/stations.csv"),
        };

        assert!(error.to_string().contains("/data/stations.csv"));
        assert_eq!(error.source().expect("a chained cause").to_string(), "permission denied");
    }
}

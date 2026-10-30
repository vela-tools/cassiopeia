use crate::csv::line_counting_reader::MalformedLine;
use ::csv::ErrorKind;
use encoding_rs::Encoding;
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

    /// The preamble or header above the CSV table's first data row could not be read.
    #[error("could not read the preamble and header of the CSV file at '{}'", path.display())]
    TableStart {
        /// The underlying CSV read error.
        #[source]
        source: ::csv::Error,
        /// The file whose table start could not be read.
        path: PathBuf,
    },

    /// A line of the file holds a byte sequence that is not valid in the encoding the file is read
    /// as, so the file is not in the encoding it was declared or detected as.
    #[error("line {line} of the CSV file is not valid {}", encoding.name())]
    Malformed {
        /// The one-based line of the file holding the malformed byte sequence.
        line: usize,
        /// The encoding the file is read as.
        encoding: &'static Encoding,
        /// The underlying CSV read error.
        #[source]
        source: ::csv::Error,
    },

    /// The underlying CSV reader failed to build or parse a record.
    #[error(transparent)]
    Reader(#[from] ::csv::Error),
}

impl CsvIngestError {
    /// The error for a read of a file in `encoding` that failed with `source`: a malformed byte
    /// sequence names its line and the encoding, and any other failure becomes `otherwise(source)`.
    pub fn reading(source: ::csv::Error, encoding: &'static Encoding, otherwise: impl FnOnce(::csv::Error) -> CsvIngestError) -> CsvIngestError {
        if let ErrorKind::Io(error) = source.kind()
            && let Some(malformed) = error.get_ref().and_then(|inner| inner.downcast_ref::<MalformedLine>())
        {
            let line = malformed.line;
            return CsvIngestError::Malformed { line, encoding, source };
        }
        otherwise(source)
    }
}

#[cfg(test)]
mod tests {
    use crate::csv::error::CsvIngestError;
    use encoding_rs::UTF_8;
    use pretty_assertions::assert_eq;
    use std::{error::Error, io, path::PathBuf};

    #[test]
    fn a_read_failure_that_is_not_a_malformed_sequence_takes_the_fallback_error() {
        let error = CsvIngestError::reading(::csv::Error::from(io::Error::other("disk gone")), UTF_8, CsvIngestError::Reader);

        assert!(matches!(error, CsvIngestError::Reader(_)));
    }

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

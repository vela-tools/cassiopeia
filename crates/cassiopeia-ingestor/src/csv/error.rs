use crate::csv::line_counting_reader::MalformedLine;
use ::csv::ErrorKind;
use encoding_rs::Encoding;
use std::{
    io,
    path::{Path, PathBuf},
};

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
    #[error("line {line} of the CSV file at '{}' is not valid {}", path.display(), encoding.name())]
    Malformed {
        /// The one-based line of the file holding the malformed byte sequence.
        line: usize,
        /// The encoding the file is read as.
        encoding: &'static Encoding,
        /// The underlying CSV read error.
        #[source]
        source: ::csv::Error,
        /// The file holding the malformed byte sequence.
        path: PathBuf,
    },

    /// The underlying CSV reader failed to read the header or a record.
    #[error("could not read the CSV file at '{}'", path.display())]
    Reader {
        /// The underlying CSV read error.
        #[source]
        source: ::csv::Error,
        /// The file that could not be read.
        path: PathBuf,
    },
}

/// Which read of a CSV file failed, for the error a failure that is no malformed byte sequence
/// becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvRead {
    /// Skipping the preamble and reading the header records above the first data row.
    TableStart,
    /// Reading the header or a data record.
    Table,
}

impl CsvIngestError {
    /// The error for the `read` of the file at `path` in `encoding` that failed with `source`: a
    /// malformed byte sequence names its line and the encoding, and any other failure is the error
    /// of that read. Every one names the file, so a run over several inputs says which one failed.
    #[must_use]
    pub fn reading(source: ::csv::Error, encoding: &'static Encoding, path: &Path, read: CsvRead) -> CsvIngestError {
        // The error outlives the ingestor's borrowed path, so it owns a copy.
        let path = path.to_path_buf();
        if let ErrorKind::Io(error) = source.kind()
            && let Some(malformed) = error.get_ref().and_then(|inner| inner.downcast_ref::<MalformedLine>())
        {
            let line = malformed.line;
            return CsvIngestError::Malformed { line, encoding, source, path };
        }
        match read {
            CsvRead::TableStart => CsvIngestError::TableStart { source, path },
            CsvRead::Table => CsvIngestError::Reader { source, path },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::csv::{
        error::{CsvIngestError, CsvRead},
        line_counting_reader::LineCountingReader,
    };
    use cassiopeia_csv_dialect::line_terminator::LineTerminator;
    use encoding_rs::UTF_8;
    use encoding_rs_rw::DecodingReader;
    use pretty_assertions::assert_eq;
    use std::{
        error::Error,
        io::{self, Read},
        path::{Path, PathBuf},
    };

    #[test]
    fn a_failed_table_read_that_is_not_a_malformed_sequence_names_the_file() {
        let error = CsvIngestError::reading(
            ::csv::Error::from(io::Error::other("disk gone")),
            UTF_8,
            Path::new("/data/stations.csv"),
            CsvRead::Table,
        );

        assert!(matches!(error, CsvIngestError::Reader { .. }));
        assert_eq!(error.to_string(), "could not read the CSV file at '/data/stations.csv'");
    }

    #[test]
    fn a_failed_table_start_read_that_is_not_a_malformed_sequence_is_a_table_start_error() {
        let error = CsvIngestError::reading(
            ::csv::Error::from(io::Error::other("disk gone")),
            UTF_8,
            Path::new("/data/stations.csv"),
            CsvRead::TableStart,
        );

        assert!(matches!(error, CsvIngestError::TableStart { .. }));
    }

    #[test]
    fn a_malformed_sequence_names_the_file_its_line_and_the_encoding() {
        let mut reader = LineCountingReader::new(DecodingReader::new(&b"id\n1\n\xff\n"[..], UTF_8.new_decoder()), LineTerminator::Lf);
        let source = ::csv::Error::from(reader.read_to_end(&mut Vec::new()).unwrap_err());
        let error = CsvIngestError::reading(source, UTF_8, Path::new("/data/stations.csv"), CsvRead::Table);

        assert_eq!(error.to_string(), "line 3 of the CSV file at '/data/stations.csv' is not valid UTF-8");
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

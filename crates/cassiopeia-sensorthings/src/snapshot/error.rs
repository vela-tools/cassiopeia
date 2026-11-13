use crate::snapshot::{
    header::{EXPECTED_HEADER, SNAPSHOT_FORMAT},
    line_number::LineNumber,
};
use serde_json::Value;
use std::io;
use thiserror::Error;

/// Why a snapshot file cannot be read.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// The first line is no snapshot header.
    #[error("the file has no SensorThings snapshot header: its first line must be one, such as {expected}", expected = EXPECTED_HEADER)]
    MissingHeader,

    /// The header names a snapshot format other than the one Cassiopeia writes.
    #[error("snapshot version {found} is not supported: Cassiopeia reads snapshot version {supported}", supported = SNAPSHOT_FORMAT)]
    UnsupportedVersion {
        /// The `sensorthingsSnapshot` value the header holds.
        found: Value,
    },

    /// A line is no JSON object of a kind the snapshot holds.
    #[error("line {line} of the snapshot is no header, datastream or observation line")]
    Malformed {
        /// The line's number.
        line: LineNumber,
        /// Why the line could not be read as its kind.
        #[source]
        source: serde_json::Error,
    },

    /// A datastream line follows an observation line.
    #[error("line {line} of the snapshot is a datastream line after an observation line, but every datastream line comes first")]
    OrderViolation {
        /// The datastream line's number.
        line: LineNumber,
    },

    /// The file could not be read, or holds text that is not UTF-8.
    #[error("the snapshot could not be read")]
    Read(#[source] io::Error),
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{error::SnapshotError, line_number::LineNumber};
    use serde_json::json;

    #[test]
    fn a_missing_header_names_the_header_expected() {
        assert!(SnapshotError::MissingHeader.to_string().contains(r#"{"sensorthingsSnapshot":1,"serviceRoot":"#));
    }

    #[test]
    fn an_unsupported_version_names_it() {
        let error = SnapshotError::UnsupportedVersion { found: json!(2) };

        assert_eq!(error.to_string(), "snapshot version 2 is not supported: Cassiopeia reads snapshot version 1");
    }

    #[test]
    fn an_order_violation_names_its_line() {
        let error = SnapshotError::OrderViolation {
            line: LineNumber::FIRST.next().next().next().next(),
        };

        assert!(error.to_string().starts_with("line 5 of the snapshot"));
    }
}

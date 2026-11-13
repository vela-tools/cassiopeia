use crate::{
    read_kind::ReadKind,
    service_root::ServiceRoot,
    snapshot::{error::SnapshotError, line_number::LineNumber},
    version::StaVersion,
};
use chrono::{DateTime, Utc};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The snapshot format this crate writes and reads, the header's `sensorthingsSnapshot` value.
pub const SNAPSHOT_FORMAT: u64 = 1;

/// The header key that names the snapshot format, and so tells a snapshot from any other JSON line.
const FORMAT_KEY: &str = "sensorthingsSnapshot";

/// A header as a snapshot starts with, quoted where a file lacks one.
pub const EXPECTED_HEADER: &str =
    r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://iot.example.org/v1.1","version":"v1.1","read":"latest","collectedAt":"2026-11-12T10:00:00Z"}"#;

/// Line 1 of a snapshot: where the readings were collected, how, and when.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters, Serialize, Deserialize)]
#[serde(try_from = "HeaderLine", into = "HeaderLine")]
pub struct SnapshotHeader {
    /// The service root the readings were read from.
    #[getset(get = "pub")]
    service_root: ServiceRoot,
    /// How the read chose the readings it took.
    #[getset(get_copy = "pub")]
    read: ReadKind,
    /// When the collection cycle started.
    #[getset(get_copy = "pub")]
    collected_at: DateTime<Utc>,
}

/// The header as it is written: the snapshot format and the root's version are stated beside the
/// members they follow from, so a reader of the file sees them without parsing the root.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeaderLine {
    sensorthings_snapshot: u64,
    service_root: ServiceRoot,
    version: StaVersion,
    read: ReadKind,
    collected_at: DateTime<Utc>,
}

/// Why a header line that names snapshot format 1 is still no header.
#[derive(Debug, Error)]
enum HeaderLineError {
    /// The header names another snapshot format.
    #[error("snapshot version {0} is not supported")]
    UnsupportedFormat(u64),
    /// The stated version differs from the one the service root ends with.
    #[error("the header states version {stated}, but its service root serves {served}")]
    VersionMismatch { stated: StaVersion, served: StaVersion },
}

impl SnapshotHeader {
    /// A header for readings read from `service_root` by a `read` read whose cycle started at
    /// `collected_at`.
    #[must_use]
    pub const fn new(service_root: ServiceRoot, read: ReadKind, collected_at: DateTime<Utc>) -> SnapshotHeader {
        SnapshotHeader {
            service_root,
            read,
            collected_at,
        }
    }

    /// Reads the header from the text of a snapshot's first line.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::MissingHeader`] when the line is no JSON object naming a snapshot
    /// format, [`SnapshotError::UnsupportedVersion`] when it names another format than
    /// [`SNAPSHOT_FORMAT`], and [`SnapshotError::Malformed`] when its members are no header's.
    pub fn from_line(line: &str) -> Result<SnapshotHeader, SnapshotError> {
        let Ok(Value::Object(members)) = serde_json::from_str::<Value>(line) else {
            return Err(SnapshotError::MissingHeader);
        };
        match members.get(FORMAT_KEY) {
            None => return Err(SnapshotError::MissingHeader),
            Some(format) if format.as_u64() != Some(SNAPSHOT_FORMAT) => {
                // The failure outlives the line it was read from.
                return Err(SnapshotError::UnsupportedVersion { found: format.clone() });
            }
            Some(_) => {}
        }
        serde_json::from_value(Value::Object(members)).map_err(|source| SnapshotError::Malformed {
            line: LineNumber::FIRST,
            source,
        })
    }
}

// Not derivable: the written line states the snapshot format and the version, which the header
// holds only through the root it was read from.
impl From<SnapshotHeader> for HeaderLine {
    fn from(header: SnapshotHeader) -> HeaderLine {
        HeaderLine {
            sensorthings_snapshot: SNAPSHOT_FORMAT,
            version: header.service_root.version(),
            service_root: header.service_root,
            read: header.read,
            collected_at: header.collected_at,
        }
    }
}

impl TryFrom<HeaderLine> for SnapshotHeader {
    type Error = HeaderLineError;

    fn try_from(line: HeaderLine) -> Result<SnapshotHeader, HeaderLineError> {
        if line.sensorthings_snapshot != SNAPSHOT_FORMAT {
            return Err(HeaderLineError::UnsupportedFormat(line.sensorthings_snapshot));
        }
        if line.version != line.service_root.version() {
            return Err(HeaderLineError::VersionMismatch {
                stated: line.version,
                served: line.service_root.version(),
            });
        }
        Ok(SnapshotHeader::new(line.service_root, line.read, line.collected_at))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        read_kind::ReadKind,
        service_root::ServiceRoot,
        snapshot::{
            error::SnapshotError,
            header::{EXPECTED_HEADER, SnapshotHeader},
        },
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use url::Url;

    fn header(read: ReadKind) -> SnapshotHeader {
        SnapshotHeader::new(
            ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap(),
            read,
            Utc.with_ymd_and_hms(2026, 11, 12, 10, 0, 0).unwrap(),
        )
    }

    #[test]
    fn the_snapshot_header_is_written_exactly() {
        assert_eq!(
            serde_json::to_string(&header(ReadKind::Latest)).unwrap(),
            r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"latest","collectedAt":"2026-11-12T10:00:00Z"}"#
        );
    }

    #[test]
    fn a_written_header_is_read_back() {
        let written = serde_json::to_string(&header(ReadKind::Range)).unwrap();

        assert_eq!(SnapshotHeader::from_line(&written).unwrap(), header(ReadKind::Range));
    }

    #[test]
    fn the_expected_header_quoted_in_errors_is_a_header() {
        assert_eq!(SnapshotHeader::from_line(EXPECTED_HEADER).unwrap().read(), ReadKind::Latest);
    }

    #[test]
    fn a_line_without_the_format_key_is_no_header() {
        for line in [r#"{"value":[]}"#, "not json", "[1]", ""] {
            assert!(matches!(SnapshotHeader::from_line(line), Err(SnapshotError::MissingHeader)), "{line}");
        }
    }

    #[test]
    fn another_snapshot_format_is_unsupported() {
        let error = SnapshotHeader::from_line(r#"{"sensorthingsSnapshot":2}"#).unwrap_err();

        assert!(matches!(error, SnapshotError::UnsupportedVersion { found } if found == json!(2)));
    }

    #[test]
    fn a_header_whose_version_differs_from_its_root_is_malformed() {
        let line = r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.0","read":"latest","collectedAt":"2026-11-12T10:00:00Z"}"#;

        let error = SnapshotHeader::from_line(line).unwrap_err();

        assert!(matches!(error, SnapshotError::Malformed { .. }), "{error}");
    }

    #[test]
    fn a_header_with_an_unknown_member_is_malformed() {
        let line = r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"latest","collectedAt":"2026-11-12T10:00:00Z","token":"x"}"#;

        assert!(matches!(SnapshotHeader::from_line(line), Err(SnapshotError::Malformed { .. })));
    }
}

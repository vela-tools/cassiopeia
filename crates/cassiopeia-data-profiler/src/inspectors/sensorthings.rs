use cassiopeia_sensorthings::snapshot::{error::SnapshotError, header::SnapshotHeader};
use std::str;

/// How far into a file its header line is looked for. A snapshot header is one short JSON object,
/// so a file whose first line runs past this is no snapshot, and a large JSON document is never
/// scanned to its end for a line break.
const HEADER_WINDOW: usize = 4 * 1024;

/// The structural inspector for a SensorThings snapshot file: it checks that the first line is a
/// snapshot header Cassiopeia reads.
///
/// It runs whichever way the format was decided: as the snapshot detector's check, and for a
/// declared `sensorthings` file, which then fails here rather than in the ingestor when its first
/// line is no header.
pub struct SensorThingsInspector;

impl SensorThingsInspector {
    /// Reads the snapshot header from the first line of `bytes`, which must end within the first
    /// 4 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::MissingHeader`] when the first line is no header or does not end
    /// within the first 4 KiB, and the [`SnapshotError`] the header reading failed with when it names
    /// a snapshot format or members Cassiopeia does not read.
    pub fn inspect(bytes: &[u8]) -> Result<SnapshotHeader, SnapshotError> {
        header_line(bytes).map_or(Err(SnapshotError::MissingHeader), SnapshotHeader::from_line)
    }
}

/// The first line of `bytes`, when it ends within [`HEADER_WINDOW`] and is UTF-8.
///
/// A file without a line break is one line, so a header-only snapshot written without its final
/// `\n` is still read.
fn header_line(bytes: &[u8]) -> Option<&str> {
    let window = bytes.get(..HEADER_WINDOW).unwrap_or(bytes);
    let line = match window.iter().position(|&byte| byte == b'\n') {
        Some(end) => window.get(..end)?,
        None if bytes.len() <= HEADER_WINDOW => window,
        None => return None,
    };
    str::from_utf8(line).ok()
}

#[cfg(test)]
mod tests {
    use crate::inspectors::sensorthings::{HEADER_WINDOW, SensorThingsInspector};
    use cassiopeia_sensorthings::{read_kind::ReadKind, snapshot::error::SnapshotError};
    use pretty_assertions::assert_eq;

    const HEADER: &str =
        r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"incremental","collectedAt":"2026-11-12T10:00:00Z"}"#;

    #[test]
    fn a_snapshot_header_is_read_from_the_first_line() {
        let snapshot = format!("{HEADER}\n{{\"datastream\":{{}}}}\n");

        assert_eq!(SensorThingsInspector::inspect(snapshot.as_bytes()).unwrap().read(), ReadKind::Incremental);
    }

    #[test]
    fn a_header_without_a_final_line_break_is_read() {
        assert!(SensorThingsInspector::inspect(HEADER.as_bytes()).is_ok());
    }

    #[test]
    fn a_first_line_that_is_no_header_is_refused() {
        let error = SensorThingsInspector::inspect(br#"{"value":[]}"#).unwrap_err();

        assert!(matches!(error, SnapshotError::MissingHeader));
        assert!(error.to_string().contains(r#"{"sensorthingsSnapshot":1,"#), "{error}");
    }

    #[test]
    fn a_first_line_past_the_header_window_is_no_header() {
        let long = format!("{HEADER}{}\n", " ".repeat(HEADER_WINDOW));

        assert!(matches!(SensorThingsInspector::inspect(long.as_bytes()), Err(SnapshotError::MissingHeader)));
    }

    #[test]
    fn an_unsupported_snapshot_version_is_named() {
        assert!(matches!(
            SensorThingsInspector::inspect(b"{\"sensorthingsSnapshot\":2}\n"),
            Err(SnapshotError::UnsupportedVersion { .. })
        ));
    }
}

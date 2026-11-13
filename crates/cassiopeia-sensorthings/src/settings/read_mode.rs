use crate::{
    read_kind::ReadKind,
    settings::{incremental_read::IncrementalRead, time_range::TimeRange},
};
use serde::{Deserialize, Serialize};
use strum::Display;

/// What a remote read collects from the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Display, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ReadMode {
    /// The newest reading of every selected data stream, one record per Thing, written `"latest"`.
    #[default]
    Latest,
    /// Every reading of a time range of every selected data stream, one record per Observation,
    /// written `{ range: { ... } }`.
    Range(TimeRange),
    /// Every reading every selected data stream gained since the last successful cycle, one record
    /// per Observation, resumed from the positions a cursor keeps, written `{ incremental: { ... } }`.
    Incremental(IncrementalRead),
}

impl ReadMode {
    /// The kind of read this mode is, as its snapshot's header states it.
    #[must_use]
    pub const fn kind(&self) -> ReadKind {
        match self {
            ReadMode::Latest => ReadKind::Latest,
            ReadMode::Range(_) => ReadKind::Range,
            ReadMode::Incremental(_) => ReadKind::Incremental,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{read_kind::ReadKind, settings::read_mode::ReadMode};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;

    fn read(json: &str) -> Result<ReadMode, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    /// The bounds of the window `read` covers in a cycle that started at 2026-11-12T10:00:00Z.
    fn bounds(read: &ReadMode) -> (DateTime<Utc>, DateTime<Utc>) {
        let ReadMode::Range(range) = read else {
            panic!("expected a range read");
        };
        let window = range.window(instant("2026-11-12T10:00:00Z")).unwrap();
        (window.from(), window.to())
    }

    #[test]
    fn a_latest_read_is_written_latest_and_states_a_latest_snapshot() {
        let read = read(r#""latest""#).unwrap();

        assert_eq!(read, ReadMode::Latest);
        assert_eq!(read.kind(), ReadKind::Latest);
        assert_eq!(read.to_string(), "latest");
    }

    #[test]
    fn range_settings_are_read() {
        assert_eq!(
            bounds(&read(r#"{"range": {"last": "24h"}}"#).unwrap()),
            (instant("2026-11-11T10:00:00Z"), instant("2026-11-12T10:00:00Z"))
        );
        assert_eq!(
            bounds(&read(r#"{"range": {"from": "2026-11-01T00:00:00Z", "to": "2026-11-02T00:00:00Z"}}"#).unwrap()),
            (instant("2026-11-01T00:00:00Z"), instant("2026-11-02T00:00:00Z"))
        );
        assert_eq!(
            bounds(&read(r#"{"range": {"from": "2026-11-01T00:00:00Z"}}"#).unwrap()),
            (instant("2026-11-01T00:00:00Z"), instant("2026-11-12T10:00:00Z"))
        );
    }

    #[test]
    fn a_range_read_states_a_range_snapshot() {
        let read = read(r#"{"range": {"last": "1h"}}"#).unwrap();

        assert_eq!(read.kind(), ReadKind::Range);
        assert_eq!(read.to_string(), "range");
        assert_eq!(serde_json::to_string(&read).unwrap(), r#"{"range":{"last":"1h"}}"#);
    }

    #[test]
    fn an_incremental_read_states_an_incremental_snapshot() {
        let read = read(r#"{"incremental": {"cursor": "rivm-air"}}"#).unwrap();

        let ReadMode::Incremental(incremental) = &read else {
            panic!("expected an incremental read");
        };
        assert_eq!(incremental.cursor().as_str(), "rivm-air");
        assert_eq!(read.kind(), ReadKind::Incremental);
        assert_eq!(read.to_string(), "incremental");
    }

    #[test]
    fn malformed_ranges_are_refused() {
        for json in [
            r#"{"range": {}}"#,
            r#"{"range": {"last": "1h", "from": "2026-11-01T00:00:00Z"}}"#,
            r#"{"range": {"to": "2026-11-02T00:00:00Z"}}"#,
            r#"{"range": {"from": "2026-11-02T00:00:00Z", "to": "2026-11-01T00:00:00Z"}}"#,
            r#"{"range": {"last": "0s"}}"#,
        ] {
            assert!(read(json).is_err(), "{json}");
        }
    }

    #[test]
    fn an_unknown_read_is_refused() {
        assert!(read(r#""newest""#).is_err());
        assert!(read(r#""range""#).is_err());
        assert!(read(r#""incremental""#).is_err());
        assert!(read(r#"{"incremental": {}}"#).is_err());
    }
}

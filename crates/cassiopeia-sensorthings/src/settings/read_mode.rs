use crate::read_kind::ReadKind;
use serde::{Deserialize, Serialize};
use strum::Display;

/// What a remote read collects from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ReadMode {
    /// The newest reading of every selected data stream, one record per Thing.
    #[default]
    Latest,
}

impl ReadMode {
    /// The kind of read this mode is, as its snapshot's header states it.
    #[must_use]
    pub const fn kind(self) -> ReadKind {
        match self {
            ReadMode::Latest => ReadKind::Latest,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{read_kind::ReadKind, settings::read_mode::ReadMode};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_latest_read_is_written_latest_and_states_a_latest_snapshot() {
        let read: ReadMode = serde_json::from_str(r#""latest""#).unwrap();

        assert_eq!(read, ReadMode::Latest);
        assert_eq!(read.kind(), ReadKind::Latest);
        assert_eq!(read.to_string(), "latest");
    }

    #[test]
    fn an_unknown_read_is_refused() {
        assert!(serde_json::from_str::<ReadMode>(r#""newest""#).is_err());
    }
}

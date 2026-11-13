use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How a SensorThings read chose the readings it took, as a snapshot's header records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ReadKind {
    /// The newest reading of every data stream.
    Latest,
    /// Every reading of a time range.
    Range,
    /// Every reading since the last read, resumed from a saved cursor.
    Incremental,
}

/// What one record of a snapshot stands for, which decides the output it can be read into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotRecords {
    /// One record per Thing, holding the newest reading of each of its data streams.
    PerThing,
    /// One record per Observation, holding that one reading. Several records then describe one
    /// Thing, so only an output that keeps every observation keeps them all.
    PerObservation,
}

impl ReadKind {
    /// What one record of a snapshot of this read stands for.
    #[must_use]
    pub const fn records(self) -> SnapshotRecords {
        match self {
            ReadKind::Latest => SnapshotRecords::PerThing,
            ReadKind::Range | ReadKind::Incremental => SnapshotRecords::PerObservation,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::read_kind::{ReadKind, SnapshotRecords};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_latest_read_gives_one_record_per_thing_and_the_others_one_per_observation() {
        assert_eq!(ReadKind::Latest.records(), SnapshotRecords::PerThing);
        assert_eq!(ReadKind::Range.records(), SnapshotRecords::PerObservation);
        assert_eq!(ReadKind::Incremental.records(), SnapshotRecords::PerObservation);
    }

    #[test]
    fn a_read_kind_is_written_in_kebab_case() {
        assert_eq!(ReadKind::Incremental.to_string(), "incremental");
        assert_eq!(serde_json::to_string(&ReadKind::Latest).unwrap(), r#""latest""#);
        assert_eq!(serde_json::from_str::<ReadKind>(r#""range""#).unwrap(), ReadKind::Range);
    }
}

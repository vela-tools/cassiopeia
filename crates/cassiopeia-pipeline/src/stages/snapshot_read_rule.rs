use crate::error::PipelineError;
use cassiopeia_common::input::Input;
use cassiopeia_manifest::output::temporal::TemporalRepresentation;
use cassiopeia_sensorthings::read_kind::{ReadKind, SnapshotRecords};

/// Refuses a SensorThings snapshot whose records the run's temporal output would not keep.
///
/// A range or incremental snapshot holds one record per Observation. A current-state store keeps one
/// record per entity and mapping, the one with the latest `observedAt` (ETSI GS CIM 009 v1.9.1
/// clause 4.8), so in current-state output such a snapshot would keep one reading of each Thing and
/// discard the rest without a word. It is read only into series output (clause 5.2.20), the rule a
/// remote range or incremental read follows. A latest snapshot holds one record per Thing, which
/// every output keeps.
///
/// The snapshot states its read only in its header, which is read once the input is profiled, so
/// the rule is applied when the snapshot's ingestor is built, before any record is read.
pub(crate) fn check_snapshot_read(read: ReadKind, temporal: Option<TemporalRepresentation>, snapshot: &Input) -> Result<(), PipelineError> {
    match (read.records(), temporal) {
        (SnapshotRecords::PerObservation, None) => Err(PipelineError::SnapshotNeedsSeries {
            read,
            // The failure outlives the ingestor that names the snapshot.
            snapshot: Box::new(snapshot.clone()),
        }),
        (SnapshotRecords::PerThing, None | Some(TemporalRepresentation::Series)) | (SnapshotRecords::PerObservation, Some(TemporalRepresentation::Series)) => {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::PipelineError, stages::snapshot_read_rule::check_snapshot_read};
    use cassiopeia_common::input::Input;
    use cassiopeia_manifest::output::temporal::TemporalRepresentation;
    use cassiopeia_sensorthings::read_kind::ReadKind;
    use std::path::PathBuf;

    fn snapshot() -> Input {
        Input::Local(PathBuf::from("/data/rivm.jsonl"))
    }

    #[test]
    fn a_latest_snapshot_is_read_into_every_output() {
        assert!(check_snapshot_read(ReadKind::Latest, None, &snapshot()).is_ok());
        assert!(check_snapshot_read(ReadKind::Latest, Some(TemporalRepresentation::Series), &snapshot()).is_ok());
    }

    #[test]
    fn a_range_or_incremental_snapshot_is_read_into_series_output() {
        assert!(check_snapshot_read(ReadKind::Range, Some(TemporalRepresentation::Series), &snapshot()).is_ok());
        assert!(check_snapshot_read(ReadKind::Incremental, Some(TemporalRepresentation::Series), &snapshot()).is_ok());
    }

    #[test]
    fn a_range_or_incremental_snapshot_into_current_state_output_is_refused_naming_its_read_and_the_setting() {
        for read in [ReadKind::Range, ReadKind::Incremental] {
            let error = check_snapshot_read(read, None, &snapshot()).unwrap_err();

            assert!(matches!(error, PipelineError::SnapshotNeedsSeries { read: refused, .. } if refused == read));
            let message = error.to_string();
            assert!(message.contains(&format!("`{read}` read")), "{message}");
            assert!(message.contains(r#"output.temporal.representation: "series""#), "{message}");
            assert!(message.contains("/data/rivm.jsonl"), "{message}");
        }
    }
}

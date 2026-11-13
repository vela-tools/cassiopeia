use crate::{
    error::{PipelineError, Result},
    input_config::{InputConfig, LaneSource},
};
use cassiopeia_common::broker_operation::BrokerOperationKind;
use cassiopeia_manifest::{
    output::{destination::Destination, temporal::TemporalRepresentation},
    schedule::Schedule,
};
use cassiopeia_sensorthings::{read_kind::SnapshotRecords, service::SensorThingsService, settings::read_mode::ReadMode};

/// Refuses a run whose inputs, destination, temporal output and schedule contradict each other,
/// when the run is built and before anything is read.
///
/// - A series run against a Context Broker needs `operation: "temporal"`: a folded `EntityTemporal`
///   only belongs at the broker's `/temporal/entities` endpoint (ETSI GS CIM 009 v1.9.1 clause
///   5.6.11).
/// - A SensorThings read that gives one record per Observation, as a range read does, needs series
///   output. A current-state store keeps one record per entity and mapping, the one with the latest
///   `observedAt` (clause 4.8), so it would keep one reading of each Thing and discard the rest.
/// - A scheduled latest SensorThings read cannot write to the temporal API. Every cycle would send the
///   newest reading of every data stream again, and the temporal API would store each unchanged
///   reading once more. A one-shot latest read into the temporal API is allowed, and so is a scheduled
///   one that upserts.
///
/// # Errors
///
/// Returns [`PipelineError::SeriesRequiresTemporalOperation`],
/// [`PipelineError::RemoteReadNeedsSeries`] and [`PipelineError::ScheduledLatestIntoTemporal`] for
/// the combinations above.
pub(crate) fn check_run_compatibility(
    inputs: &[InputConfig],
    destination: &Destination,
    temporal: Option<TemporalRepresentation>,
    schedule: Option<&Schedule>,
) -> Result<()> {
    let into_temporal_api = matches!(destination, Destination::ContextBroker { operation, .. } if *operation == BrokerOperationKind::Temporal);
    if temporal == Some(TemporalRepresentation::Series) && matches!(destination, Destination::ContextBroker { .. }) && !into_temporal_api {
        return Err(PipelineError::SeriesRequiresTemporalOperation);
    }
    if temporal.is_none()
        && let Some(service) = inputs.iter().find_map(|input| per_observation_read(&input.source))
    {
        return Err(PipelineError::RemoteReadNeedsSeries {
            read: service.settings().read().kind(),
            // The error names the root, while the lane keeps its own.
            root: Box::new(service.root().clone()),
        });
    }
    if schedule.is_some()
        && into_temporal_api
        && let Some(service) = inputs.iter().find_map(|input| latest_read(&input.source))
    {
        return Err(PipelineError::ScheduledLatestIntoTemporal {
            // The error names the root, while the lane keeps its own.
            root: Box::new(service.root().clone()),
        });
    }
    Ok(())
}

/// The SensorThings server a lane reads its latest readings from, if it reads one.
fn latest_read(source: &LaneSource) -> Option<&SensorThingsService> {
    match source {
        LaneSource::SensorThings(service) => match service.settings().read() {
            ReadMode::Latest => Some(service),
            ReadMode::Range(_) => None,
        },
        LaneSource::Files(_) => None,
    }
}

/// The SensorThings server a lane reads one record per Observation from, if it reads one.
fn per_observation_read(source: &LaneSource) -> Option<&SensorThingsService> {
    match source {
        LaneSource::SensorThings(service) => match service.settings().read().kind().records() {
            SnapshotRecords::PerObservation => Some(service),
            SnapshotRecords::PerThing => None,
        },
        LaneSource::Files(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::PipelineError,
        input_config::{InputConfig, build_input_config},
        run_compatibility::check_run_compatibility,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};
    use cassiopeia_manifest::{
        input::ManifestInput,
        output::{ManifestOutput, destination::Destination, temporal::TemporalRepresentation},
        schedule::{Schedule, trigger::Trigger},
    };
    use cassiopeia_sensorthings::read_kind::ReadKind;
    use pretty_assertions::assert_eq;
    use serde_json::Map;
    use std::time::Duration;

    /// The lane of the input `json`.
    fn lane(json: &str) -> InputConfig {
        build_input_config(&serde_json::from_str::<ManifestInput>(json).unwrap(), &Map::new()).unwrap()
    }

    /// A lane that reads the latest readings of a SensorThings server.
    fn latest_lane() -> InputConfig {
        lane(r#"{"source": "https://h.example/v1.1", "format": "sensorthings", "mapping": "m.json5"}"#)
    }

    /// A broker destination with the operation `operation`.
    fn broker(operation: &str) -> Destination {
        let output: ManifestOutput = serde_json::from_str(&format!(
            r#"{{"target": "context-broker", "url": "http://localhost:1026/", "operation": "{operation}"}}"#
        ))
        .unwrap();
        output.destination().clone()
    }

    fn every_five_minutes() -> Schedule {
        Schedule::builder().trigger(Trigger::Every(Duration::from_mins(5))).build()
    }

    const SERIES: Option<TemporalRepresentation> = Some(TemporalRepresentation::Series);

    #[test]
    fn series_against_a_broker_requires_the_temporal_operation() {
        let result = check_run_compatibility(&[latest_lane()], &broker("upsert"), SERIES, None);

        assert!(matches!(result, Err(PipelineError::SeriesRequiresTemporalOperation)), "{result:?}");
    }

    #[test]
    fn a_scheduled_latest_read_into_the_temporal_api_is_refused() {
        let error = check_run_compatibility(&[latest_lane()], &broker("temporal"), SERIES, Some(&every_five_minutes())).unwrap_err();

        assert!(
            matches!(&error, PipelineError::ScheduledLatestIntoTemporal { root } if root.url().as_str() == "https://h.example/v1.1"),
            "{error:?}"
        );
        assert!(error.to_string().contains("would store each unchanged reading once more"), "{error}");
    }

    #[test]
    fn a_one_shot_latest_read_into_the_temporal_api_is_allowed() {
        check_run_compatibility(&[latest_lane()], &broker("temporal"), SERIES, None).unwrap();
    }

    #[test]
    fn a_scheduled_latest_upsert_is_allowed() {
        check_run_compatibility(&[latest_lane()], &broker("upsert"), None, Some(&every_five_minutes())).unwrap();
    }

    #[test]
    fn a_scheduled_snapshot_file_into_the_temporal_api_is_allowed() {
        let snapshot = lane(r#"{"source": "rivm.jsonl", "format": "sensorthings", "mapping": "m.json5"}"#);

        check_run_compatibility(&[snapshot], &broker("temporal"), SERIES, Some(&every_five_minutes())).unwrap();
    }

    /// A lane that reads the last day of readings of a SensorThings server.
    fn range_lane() -> InputConfig {
        lane(r#"{"source": "https://h.example/v1.1", "format": "sensorthings", "mapping": "m.json5", "sensorthings": {"read": {"range": {"last": "24h"}}}}"#)
    }

    fn file_output() -> Destination {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file"}"#).unwrap();
        output.destination().clone()
    }

    #[test]
    fn a_range_read_without_series_is_refused() {
        let error = check_run_compatibility(&[range_lane()], &file_output(), None, None).unwrap_err();

        assert!(
            matches!(&error, PipelineError::RemoteReadNeedsSeries { read: ReadKind::Range, root } if root.url().as_str() == "https://h.example/v1.1"),
            "{error:?}"
        );
        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Run(RunCode::Misconfigured));
        let message = error.to_string();
        assert!(message.contains("`range` read"), "{message}");
        assert!(message.contains(r#"output.temporal.representation: "series""#), "{message}");
    }

    #[test]
    fn a_range_read_into_series_files_is_allowed() {
        check_run_compatibility(&[range_lane()], &file_output(), SERIES, None).unwrap();
    }

    #[test]
    fn a_scheduled_range_read_into_the_temporal_api_is_allowed() {
        check_run_compatibility(&[range_lane()], &broker("temporal"), SERIES, Some(&every_five_minutes())).unwrap();
    }

    #[test]
    fn a_latest_read_without_series_is_allowed() {
        check_run_compatibility(&[latest_lane()], &file_output(), None, None).unwrap();
    }

    #[test]
    fn series_into_files_needs_no_broker_operation() {
        check_run_compatibility(&[latest_lane()], &file_output(), SERIES, Some(&every_five_minutes())).unwrap();
    }
}

use crate::{
    end_to_end::harness::{RunOutcome, RunSchemas, STORES, attempt_file_end_to_end, entity_ids},
    error::PipelineError,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};
use cassiopeia_sensorthings::read_kind::ReadKind;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use temp_dir::TempDir;

/// The ids RIVM Samen Meten gives the two Things of its fixtures.
const RIVM_THINGS: [&str; 2] = [
    "urn:ngsi-ld:AirQualityObserved:sta-api-samenmetenrivmnl-12511",
    "urn:ngsi-ld:AirQualityObserved:sta-api-samenmetenrivmnl-12512",
];

/// The instant every reading of the RIVM latest fixture was made.
const RIVM_LATEST_TIME: &str = "2026-11-13T07:00:00Z";

/// The output settings of a run that keeps every observation.
const SERIES_OUTPUT: &str = r#", temporal: { representation: "series" }"#;

/// The fixture `name` of the SensorThings snapshot tests.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sensorthings").join(name)
}

/// Runs the snapshot fixture `snapshot` through the `AirQualityObserved` mapping on `store`, with
/// `output_extra` spliced into the output, validating against the Smart Data Models
/// `AirQualityObserved` schema.
fn run_snapshot(store: StoreKind, snapshot: &Path, output_extra: &str) -> (Result<(), PipelineError>, RunOutcome) {
    attempt_file_end_to_end(
        store,
        snapshot,
        &fixture("air-quality-observed.json5"),
        &RunSchemas::Folder(&fixture("schemas")),
        output_extra,
    )
}

/// The written entity `id`.
fn entity<'a>(outcome: &'a RunOutcome, id: &str) -> &'a Value {
    outcome.entities.iter().find(|entity| entity["id"] == id).expect("the entity is written")
}

/// The `observedAt` of every instance of a series attribute, in the order written.
fn instant_times(attribute: &Value) -> Vec<&str> {
    attribute
        .as_array()
        .expect("a series attribute is an array of instances")
        .iter()
        .map(|instance| instance["observedAt"].as_str().expect("an instance carries its observedAt"))
        .collect()
}

#[test]
fn rivm_latest_maps_to_validated_air_quality() {
    for store in STORES {
        let (result, outcome) = run_snapshot(store, &fixture("rivm-v1.0-latest.jsonl"), r#", validation: { mode: "fail" }"#);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), RIVM_THINGS, "{store:?}");
        for id in RIVM_THINGS {
            let station = entity(&outcome, id);
            for attribute in ["pm10", "pm25"] {
                assert_eq!(station[attribute]["observedAt"], RIVM_LATEST_TIME, "{store:?} {id} {attribute}");
                assert_eq!(station[attribute]["unitCode"], "GQ", "{store:?} {id} {attribute}");
                assert!(station[attribute]["value"].is_number(), "{store:?} {id} {attribute}");
            }
            assert_eq!(station["location"]["value"]["type"], "Point", "{store:?} {id}");
            assert!(
                station["dateObserved"].to_string().contains(RIVM_LATEST_TIME),
                "{store:?} {id}: {}",
                station["dateObserved"]
            );
        }
        assert_eq!(entity(&outcome, RIVM_THINGS[1])["pm10"]["value"], 9.46, "{store:?}");
        assert_eq!(entity(&outcome, RIVM_THINGS[1])["pm25"]["value"], 6.77, "{store:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn frost_latest_with_feature_locations_and_string_ids() {
    let things = [
        ("urn:ngsi-ld:AirQualityObserved:sta-localhost-eindhoven-centrum", [5.4797, 51.4392], 21.7, 11.3),
        ("urn:ngsi-ld:AirQualityObserved:sta-localhost-helmond-west", [5.64, 51.475], 14.1, 7.6),
    ];
    for store in STORES {
        let (result, outcome) = run_snapshot(store, &fixture("frost-v1.1-latest.jsonl"), r#", validation: { mode: "fail" }"#);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), things.map(|(id, ..)| id), "{store:?}");
        for (id, coordinates, pm10, pm25) in things {
            let station = entity(&outcome, id);
            assert_eq!(
                station["location"]["value"],
                json!({"type": "Point", "coordinates": coordinates}),
                "{store:?} {id}"
            );
            assert_eq!(station["pm10"]["value"], pm10, "{store:?} {id}");
            assert_eq!(station["pm25"]["value"], pm25, "{store:?} {id}");
            for attribute in ["pm10", "pm25"] {
                // Each reading's time is the end of its hourly interval, and its unit symbol `µg/m³`
                // translates to the UN/CEFACT code of microgram per cubic metre.
                assert_eq!(station[attribute]["observedAt"], "2026-11-13T07:00:00Z", "{store:?} {id} {attribute}");
                assert_eq!(station[attribute]["unitCode"], "GQ", "{store:?} {id} {attribute}");
            }
        }
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn a_latest_read_into_series_files_folds_each_attribute_once() {
    for store in STORES {
        let (result, outcome) = run_snapshot(store, &fixture("rivm-v1.0-latest.jsonl"), SERIES_OUTPUT);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), RIVM_THINGS, "{store:?}");
        for id in RIVM_THINGS {
            for attribute in ["pm10", "pm25"] {
                assert_eq!(
                    instant_times(&entity(&outcome, id)[attribute]),
                    [RIVM_LATEST_TIME],
                    "{store:?} {id} {attribute}"
                );
            }
        }
    }
}

#[test]
fn a_range_read_into_series_files_sorts_instances_oldest_first() {
    for store in STORES {
        let (result, outcome) = run_snapshot(store, &fixture("rivm-v1.0-range.jsonl"), SERIES_OUTPUT);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), [RIVM_THINGS[1]], "{store:?}");
        let station = entity(&outcome, RIVM_THINGS[1]);
        assert_eq!(
            instant_times(&station["pm10"]),
            ["2026-11-12T04:00:00Z", "2026-11-12T05:00:00Z", "2026-11-12T06:00:00Z"],
            "{store:?}"
        );
        assert_eq!(
            instant_times(&station["pm25"]),
            ["2026-11-12T03:00:00Z", "2026-11-12T04:00:00Z", "2026-11-12T05:00:00Z"],
            "{store:?}"
        );
    }
}

#[test]
fn a_range_snapshot_into_current_state_output_is_refused() {
    for store in STORES {
        let (result, outcome) = run_snapshot(store, &fixture("rivm-v1.0-range.jsonl"), "");

        let error = result.unwrap_err();
        assert!(
            matches!(error, PipelineError::SnapshotNeedsSeries { read: ReadKind::Range, .. }),
            "{store:?}: {error}"
        );
        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Run(RunCode::Misconfigured), "{store:?}");
        assert!(error.to_string().contains(r#"output.temporal.representation: "series""#), "{store:?}: {error}");
        assert!(outcome.entities.is_empty(), "{store:?}");
    }
}

#[test]
fn an_incremental_snapshot_into_current_state_output_is_refused() {
    let directory = TempDir::new().unwrap();
    let snapshot = directory.path().join("rivm-incremental.jsonl");
    let range = fs::read_to_string(fixture("rivm-v1.0-range.jsonl")).unwrap();
    fs::write(&snapshot, range.replacen(r#""read":"range""#, r#""read":"incremental""#, 1)).unwrap();

    for store in STORES {
        let (result, outcome) = run_snapshot(store, &snapshot, "");

        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                PipelineError::SnapshotNeedsSeries {
                    read: ReadKind::Incremental,
                    ..
                }
            ),
            "{store:?}: {error}"
        );
        assert!(error.to_string().contains("`incremental` read"), "{store:?}: {error}");
        assert!(outcome.entities.is_empty(), "{store:?}");
    }
}

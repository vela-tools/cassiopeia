use crate::{
    end_to_end::{
        harness::{STORES, attempt_inputs_end_to_end},
        sensorthings::fixture,
        station_mappings::STATION_NAME,
    },
    error::PipelineError,
};
use cassiopeia_sensorthings::read_kind::ReadKind;
use std::{error::Error, fs, iter::successors, path::Path};
use temp_dir::TempDir;

/// The entity type the SensorThings fixture mapping writes, given an accept-anything schema.
const AIR_QUALITY_OBSERVED: &[&str] = &["dataModel.Environment/AirQualityObserved"];

/// The manifest input that reads `source`, bound to the mapping at `mapping`.
fn declared(source: &Path, mapping: &Path) -> String {
    format!(r#"{{ source: "{}", mapping: "{}" }}"#, source.display(), mapping.display())
}

/// `error` and every error under it, joined as one line, so a test can find a path any of them names.
fn rendered(error: &PipelineError) -> String {
    successors(Some(error as &dyn Error), |&error| error.source())
        .map(ToString::to_string)
        .collect::<Vec<String>>()
        .join(": ")
}

#[test]
fn two_snapshots_current_state_output_refuses_fail_the_run_naming_the_one_the_manifest_lists_first() {
    let mapping = fixture("air-quality-observed.json5");
    for (first, second) in [
        ("frost-v1.1-range.jsonl", "rivm-v1.0-range.jsonl"),
        ("rivm-v1.0-range.jsonl", "frost-v1.1-range.jsonl"),
    ] {
        let inputs = [declared(&fixture(first), &mapping), declared(&fixture(second), &mapping)];
        for store in STORES {
            let (result, outcome) = attempt_inputs_end_to_end(store, &inputs, AIR_QUALITY_OBSERVED);

            let error = result.unwrap_err();
            assert!(
                matches!(error, PipelineError::SnapshotNeedsSeries { read: ReadKind::Range, .. }),
                "{store:?} {first}: {error}"
            );
            assert!(error.to_string().contains(first), "{store:?} {first}: {error}");
            assert!(outcome.entities.is_empty(), "{store:?} {first}");
        }
    }
}

#[test]
fn two_missing_files_fail_the_run_naming_the_one_the_manifest_lists_first() {
    let directory = TempDir::new().unwrap();
    let mapping = directory.path().join("stations.json5");
    fs::write(&mapping, STATION_NAME).unwrap();
    let [north, south] = ["north.csv", "south.csv"].map(|name| directory.path().join(name));
    for (first, second) in [(&north, &south), (&south, &north)] {
        let inputs = [declared(first, &mapping), declared(second, &mapping)];
        for store in STORES {
            let (result, outcome) = attempt_inputs_end_to_end(store, &inputs, &["Station"]);

            let error = rendered(&result.unwrap_err());
            assert!(error.contains(&first.display().to_string()), "{store:?}: {error}");
            assert!(!error.contains(&second.display().to_string()), "{store:?}: {error}");
            assert!(outcome.entities.is_empty(), "{store:?}");
        }
    }
}

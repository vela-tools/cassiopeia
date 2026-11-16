use crate::end_to_end::{
    harness::{RunOutcome, RunSchemas, STORES, attempt_file_end_to_end, entity_ids},
    sensorthings::{entity, fixture},
};
use cassiopeia_common::store_kind::StoreKind;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path};
use temp_dir::TempDir;

/// The schemas the station mapping validates against: the run checks what the mapping writes, not
/// what a `Device` schema requires of it.
const DEVICE_SCHEMAS: RunSchemas<'static> = RunSchemas::Permissive(&["dataModel.Device/Device"]);

/// The station whose `pm10` Datastream is served with a `phenomenonTime`, as the mapping writes it.
const REPORTED: &str = "urn:ngsi-ld:Device:sta-hexample-1";

/// The station whose `pm10` Datastream is served without a `phenomenonTime`, as the mapping writes it.
const UNREPORTED: &str = "urn:ngsi-ld:Device:sta-hexample-2";

/// Runs the snapshot `snapshot` through the mapping that writes when each station's `pm10` data
/// stream last reported, on `store`.
fn run(store: StoreKind, snapshot: &Path) -> RunOutcome {
    let (result, outcome) = attempt_file_end_to_end(store, snapshot, &fixture("station-last-reported.json5"), &DEVICE_SCHEMAS, "");
    result.unwrap();
    outcome
}

/// The data stream line of a latest read of the `pm10` Datastream of Thing `thing`, with its
/// newest Observation, served with the `phenomenonTime` `phenomenon_time` when there is one.
fn pm10(thing: u64, phenomenon_time: Option<&str>) -> Value {
    let mut datastream = json!({
        "@iot.id": thing * 10,
        "name": format!("station-{thing} pm10"),
        "unitOfMeasurement": {"symbol": "ug/m3"},
        "Thing": {"@iot.id": thing, "name": format!("Station {thing}")},
        "ObservedProperty": {"@iot.id": 1, "name": "pm10"},
        "Observations": [{"@iot.id": thing * 100, "phenomenonTime": "2026-11-12T10:00:00Z", "result": 21.4}],
    });
    if let Some(time) = phenomenon_time {
        datastream["phenomenonTime"] = Value::from(time);
    }
    json!({ "datastream": datastream })
}

#[test]
fn a_station_last_reported_at_the_end_of_its_data_stream_s_phenomenon_time() {
    let folder = TempDir::new().unwrap();
    let snapshot = folder.path().join("stations.jsonl");
    let header =
        json!({"sensorthingsSnapshot": 1, "serviceRoot": "https://h.example/v1.1", "version": "v1.1", "read": "latest", "collectedAt": "2026-11-12T10:30:00Z"});
    let lines = [header, pm10(1, Some("2026-11-01T00:00:00Z/2026-11-12T10:00:00Z")), pm10(2, None)];
    fs::write(&snapshot, lines.map(|line| format!("{line}\n")).concat()).unwrap();

    for store in STORES {
        let outcome = run(store, &snapshot);

        assert_eq!(entity_ids(&outcome), [REPORTED, UNREPORTED], "{store:?}");
        assert_eq!(
            entity(&outcome, REPORTED)["dateLastValueReported"]["value"],
            "2026-11-12T10:00:00Z",
            "{store:?}"
        );
        assert_eq!(entity(&outcome, UNREPORTED).get("dateLastValueReported"), None, "{store:?}");
        assert!(outcome.diagnostics.is_empty(), "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn a_frost_snapshot_recorded_before_the_members_were_read_gives_each_station_its_last_report() {
    let stations = [
        "urn:ngsi-ld:Device:sta-localhost-eindhoven-centrum",
        "urn:ngsi-ld:Device:sta-localhost-helmond-west",
    ];
    for store in STORES {
        let outcome = run(store, &fixture("frost-v1.1-latest.jsonl"));

        assert_eq!(entity_ids(&outcome), stations, "{store:?}");
        for station in stations {
            // FROST-Server serves each Datastream's `phenomenonTime` as the interval from the start
            // of its oldest Observation to the end of its newest.
            assert_eq!(
                entity(&outcome, station)["dateLastValueReported"]["value"],
                "2026-11-13T07:00:00Z",
                "{store:?} {station}"
            );
        }
        assert!(outcome.diagnostics.is_empty(), "{store:?}: {:?}", outcome.diagnostics);
    }
}

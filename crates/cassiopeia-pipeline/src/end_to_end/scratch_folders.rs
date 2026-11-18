//! Every attempt at a cycle downloads into a scratch folder of its own in the temporary folder and
//! removes it when it ends, so a run leaves the temporary folder as it found it, whether its cycles
//! completed, failed or were stopped.

use crate::{
    end_to_end::{
        harness::{RunBinding, RunSchemas, RunSetup, RunStop, attempt_declared_end_to_end, attempt_source_end_to_end, entity_ids, run_csv_end_to_end},
        sensorthings::fixture,
        station_mappings::STATION_NAME,
    },
    error::PipelineError,
};
use cassiopeia_collector::error::CollectorError;
use cassiopeia_common::store_kind::StoreKind;
use mockito::{Mock, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{path::PathBuf, sync::atomic::AtomicBool};

/// The flag the harness raises once the third cycle of the scheduled test has ended.
static STOPPED_AFTER_THIRD_CYCLE: AtomicBool = AtomicBool::new(false);

/// The CSV document every CSV input reads: two stations.
const STATIONS: &str = "id,name\nS1,Centrum\nS2,Haven\n";

/// The entities the stations are written as.
const STATION_IDS: [&str; 2] = ["urn:ngsi-ld:Station:S1", "urn:ngsi-ld:Station:S2"];

/// The path and query of the latest discovery request with the default settings.
const LATEST: &str =
    "/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc";

/// The schemas the station inventory validates against.
const INVENTORY_SCHEMAS: RunSchemas<'static> = RunSchemas::Permissive(&["dataModel.Device/Device"]);

/// Serves the stations at `/stations.csv`, `downloads` times.
fn serve_stations(server: &mut ServerGuard, downloads: usize) -> Mock {
    server.mock("GET", "/stations.csv").with_body(STATIONS).expect(downloads).create()
}

/// The URL the mock server `server` serves the stations at.
fn stations_url(server: &ServerGuard) -> String {
    format!("{}/stations.csv", server.url())
}

/// The manifest input that reads the newest readings of the mock server `server` into the station
/// inventory.
fn latest_read(server: &ServerGuard) -> String {
    format!(
        r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}" }}"#,
        server.url(),
        fixture("station-inventory.json5").display()
    )
}

#[test]
fn a_remote_csv_is_downloaded_into_the_scratch_folder_and_leaves_the_temporary_folder_empty() {
    let mut server = mockito::Server::new();
    let download = serve_stations(&mut server, 1);
    let schemas = RunSchemas::Permissive(&["Station"]);

    let (result, outcome) = attempt_source_end_to_end(
        StoreKind::DashMap,
        &stations_url(&server),
        &RunBinding::Mapping(STATION_NAME),
        &RunSetup::files(&schemas, ""),
    );

    result.unwrap();
    download.assert();
    assert_eq!(entity_ids(&outcome), STATION_IDS);
    // An input that declares no format is downloaded under the extension of no format, and detected.
    assert_eq!(outcome.temporary_files_read, [["input-0.dat"]]);
    assert_eq!(outcome.temporary_entries_after_cycles, [Vec::<PathBuf>::new()]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_local_csv_writes_nothing_into_the_scratch_folder_and_leaves_no_folder_behind() {
    let outcome = run_csv_end_to_end(StoreKind::DashMap, STATIONS, &RunBinding::Mapping(STATION_NAME), &["Station"], "");

    assert_eq!(entity_ids(&outcome), STATION_IDS);
    assert_eq!(outcome.temporary_files_read, [Vec::<String>::new()]);
    assert_eq!(outcome.temporary_entries_after_cycles, [Vec::<PathBuf>::new()]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_sensorthings_read_whose_cycle_completed_leaves_no_snapshot_behind() {
    let mut server = mockito::Server::new();
    let station = json!({
        "@iot.id": 11,
        "name": "station-1 pm10",
        "unitOfMeasurement": { "symbol": "ug/m3" },
        "Thing": { "@iot.id": 1, "name": "Stationsplein", "description": "Air quality station Stationsplein", "Locations": [] },
        "ObservedProperty": { "@iot.id": 1, "name": "pm10" },
        "Observations": [{ "@iot.id": 501, "phenomenonTime": "2026-11-16T12:00:00Z", "result": 18.2 }],
    });
    let discovery = server
        .mock("GET", LATEST)
        .with_body(json!({ "value": [station] }).to_string())
        .expect(1)
        .create();

    let (result, outcome) = attempt_declared_end_to_end(StoreKind::DashMap, &latest_read(&server), &INVENTORY_SCHEMAS, "");

    result.unwrap();
    discovery.assert();
    assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Device:sta-127001-1"]);
    assert_eq!(outcome.temporary_files_read, [["input-0.jsonl"]]);
    assert_eq!(outcome.temporary_entries_after_cycles, [Vec::<PathBuf>::new()]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_sensorthings_read_the_server_fails_with_500_leaves_no_snapshot_behind() {
    let mut server = mockito::Server::new();
    // The downloader asks a server that answers `500` three times before the answer is final.
    let discovery = server.mock("GET", LATEST).with_status(500).expect(3).create();

    let (result, outcome) = attempt_declared_end_to_end(StoreKind::DashMap, &latest_read(&server), &INVENTORY_SCHEMAS, "");

    let error = result.unwrap_err();
    assert!(matches!(&error, PipelineError::Collector(CollectorError::HttpStatus { .. })), "{error:?}");
    discovery.assert();
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_scheduled_remote_csv_stopped_after_three_cycles_leaves_the_temporary_folder_empty_after_every_cycle() {
    let mut server = mockito::Server::new();
    let downloads = serve_stations(&mut server, 3);
    let schemas = RunSchemas::Permissive(&["Station"]);
    let setup = RunSetup {
        schedule: Some(r#"{ mode: "every", value: "1s" }"#),
        stop: RunStop::after_cycles(&STOPPED_AFTER_THIRD_CYCLE, 3),
        ..RunSetup::files(&schemas, "")
    };

    let (result, outcome) = attempt_source_end_to_end(StoreKind::DashMap, &stations_url(&server), &RunBinding::Mapping(STATION_NAME), &setup);

    result.unwrap();
    downloads.assert();
    // Each cycle finds the one download it made itself: the cycle before it left nothing behind.
    assert_eq!(outcome.temporary_files_read, [["input-0.dat"], ["input-0.dat"], ["input-0.dat"]]);
    assert_eq!(outcome.temporary_entries_after_cycles, vec![Vec::<PathBuf>::new(); 3]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

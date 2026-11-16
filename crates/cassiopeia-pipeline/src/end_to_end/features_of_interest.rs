use crate::end_to_end::{
    harness::{RunOutcome, RunSchemas, RunSetup, STORES, attempt_declared_end_to_end, attempt_file_end_to_end, attempt_setup_end_to_end, entity_ids},
    sensorthings::{SERIES_OUTPUT, entity, fixture, instant_times},
};
use cassiopeia_common::store_kind::StoreKind;
use mockito::{Matcher, Mock, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::fs;
use temp_dir::TempDir;

/// The places the bus of the moving-Thing fixture took its three readings at, in time order.
const STOPS: [[f64; 2]; 3] = [[5.4797, 51.4433], [5.4812, 51.4402], [5.4870, 51.4381]];

/// The times the bus of the moving-Thing fixture took its three readings at.
const READING_TIMES: [&str; 3] = ["2026-11-13T08:00:00Z", "2026-11-13T08:10:00Z", "2026-11-13T08:20:00Z"];

/// The path and query of the metadata discovery request of the mock server's `/v1.1` root.
const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000";

/// The path and query of the latest discovery request of the mock server's `/v1.1` root that
/// requests features of interest.
const LATEST_WITH_FEATURES: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc&$top=1000";

/// The entity the bus is written as when read from the mock server.
const MOCK_BUS: &str = "urn:ngsi-ld:AirQualityObserved:sta-127001-bus-12";

/// A `GeoJSON` point at `coordinates`.
fn point(coordinates: [f64; 2]) -> Value {
    json!({"type": "Point", "coordinates": coordinates})
}

/// The Datastream and the Observations of the moving-Thing fixture, as a server serves them.
fn moving_thing() -> (Value, Vec<Value>) {
    let text = fs::read_to_string(fixture("frost-v1.1-moving-range.jsonl")).unwrap();
    let mut lines = text.lines().skip(1).map(|line| serde_json::from_str::<Value>(line).unwrap());
    let datastream = lines.next().unwrap()["datastream"].take();
    let observations = lines.map(|mut line| line["observation"].take()).collect();
    (datastream, observations)
}

/// The values and the `observedAt` of every instance of the series attribute `location` of `station`.
fn located_readings(station: &Value) -> (Vec<&Value>, Vec<&str>) {
    let values = station["location"]
        .as_array()
        .expect("a series attribute is an array of instances")
        .iter()
        .map(|instance| &instance["value"])
        .collect();
    (values, instant_times(&station["location"]))
}

/// The input that reads the mock server's `/v1.1` root through the moving-Thing mapping, with the
/// `sensorthings` members `settings`.
fn remote(server: &ServerGuard, settings: &str) -> String {
    format!(
        r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}", sensorthings: {{ {settings}, featuresOfInterest: true }} }}"#,
        server.url(),
        fixture("moving-air-quality-observed.json5").display()
    )
}

/// Runs the latest read of the mock server, which answers the latest discovery that requests features
/// of interest with `datastream`, and only that request, into current-state files, validating in
/// `warn` mode, so an entity without a `location`, which `AirQualityObserved` requires, is still
/// written.
fn latest_run(datastream: &Value) -> RunOutcome {
    let mut server = mockito::Server::new();
    let discovery = server
        .mock("GET", LATEST_WITH_FEATURES)
        .with_body(json!({ "value": [datastream] }).to_string())
        .expect(1)
        .create();

    let (result, outcome) = attempt_declared_end_to_end(
        StoreKind::DashMap,
        &remote(&server, r#"read: "latest""#),
        &RunSchemas::Folder(&fixture("schemas")),
        r#", validation: { mode: "warn" }"#,
    );

    result.unwrap();
    discovery.assert();
    outcome
}

#[test]
fn a_moving_thing_is_written_with_one_location_per_reading() {
    for store in STORES {
        let (result, outcome) = attempt_file_end_to_end(
            store,
            &fixture("frost-v1.1-moving-range.jsonl"),
            &fixture("moving-air-quality-observed.json5"),
            &RunSchemas::Folder(&fixture("schemas")),
            SERIES_OUTPUT,
        );
        result.unwrap();

        let bus = "urn:ngsi-ld:AirQualityObserved:sta-localhost-bus-12";
        assert_eq!(entity_ids(&outcome), [bus], "{store:?}");
        let (values, times) = located_readings(entity(&outcome, bus));
        assert_eq!(values, STOPS.map(point).iter().collect::<Vec<_>>(), "{store:?}");
        assert_eq!(times, READING_TIMES, "{store:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn an_incremental_read_places_each_reading_where_it_was_taken() {
    let (datastream, observations) = moving_thing();
    let mut server = mockito::Server::new();
    let mocks = [
        server
            .mock("GET", METADATA)
            .with_body(json!({ "value": [datastream] }).to_string())
            .expect(1)
            .create(),
        server
            .mock(
                "GET",
                Matcher::Regex(
                    r"^/v1\.1/Datastreams\('bus-12-pm10'\)/Observations\?\$filter=[^&]+&\$orderby=phenomenonTime%20asc,id%20asc&\$expand=FeatureOfInterest&\$top=1000$"
                        .to_owned(),
                ),
            )
            .with_body(json!({ "value": observations }).to_string())
            .expect(1)
            .create(),
    ];
    let cursors = TempDir::new().unwrap();
    let folder = fixture("schemas");
    let schemas = RunSchemas::Folder(&folder);
    let setup = RunSetup {
        cursors: Some(cursors.path()),
        ..RunSetup::files(&schemas, SERIES_OUTPUT)
    };

    let (result, outcome) = attempt_setup_end_to_end(
        StoreKind::DashMap,
        &remote(&server, r#"read: { incremental: { cursor: "bus-12", initial: "2026-01-01T00:00:00Z" } }"#),
        &setup,
    );

    result.unwrap();
    mocks.iter().for_each(Mock::assert);
    let (values, times) = located_readings(entity(&outcome, MOCK_BUS));
    assert_eq!(values, STOPS.map(point).iter().collect::<Vec<_>>());
    assert_eq!(times, READING_TIMES);
}

#[test]
fn a_latest_read_places_the_newest_reading_where_it_was_taken() {
    let (mut datastream, observations) = moving_thing();
    datastream["Observations"] = json!([observations.last().unwrap()]);

    let outcome = latest_run(&datastream);

    let bus = entity(&outcome, MOCK_BUS);
    assert_eq!(bus["location"]["value"], point(STOPS[2]));
    assert_eq!(bus["location"]["observedAt"], READING_TIMES[2]);
    assert_eq!(outcome.warnings, 0, "{:?}", outcome.diagnostics);
}

#[test]
fn a_latest_read_of_a_server_that_ignores_the_nested_expansion_succeeds_without_the_place() {
    let (mut datastream, observations) = moving_thing();
    let mut newest = observations.last().unwrap().clone();
    newest.as_object_mut().unwrap().remove("FeatureOfInterest");
    datastream["Observations"] = json!([newest]);

    let outcome = latest_run(&datastream);

    let bus = entity(&outcome, MOCK_BUS);
    assert!(bus.get("location").is_none(), "{bus}");
    assert_eq!(bus["pm10"]["observedAt"], READING_TIMES[2]);
}

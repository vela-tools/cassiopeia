use crate::end_to_end::{
    harness::{RunOutcome, RunSchemas, RunSetup, STORES, attempt_file_end_to_end, attempt_setup_end_to_end, entity_ids},
    sensorthings::{SERIES_OUTPUT, entity, fixture, instant_times},
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode};
use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, read_stream::ReadStream, service_root::ServiceRoot};
use cassiopeia_sensorthings_cursor::{cursor_store::CursorStore, position::Position};
use chrono::{DateTime, Utc};
use mockito::{Matcher, Mock, Request, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};
use temp_dir::TempDir;
use url::Url;

/// The entity the bus of the track fixture is written as.
const BUS: &str = "urn:ngsi-ld:AirQualityObserved:sta-hexample-12";

/// The places the track fixture's `HistoricalLocations` hold, oldest first.
const TRACK: [[f64; 2]; 3] = [[5.4797, 51.4433], [5.4812, 51.4402], [5.4870, 51.4381]];

/// The times of the track fixture's `HistoricalLocations`, oldest first.
const TRACK_TIMES: [&str; 3] = ["2026-11-16T07:58:00Z", "2026-11-16T08:08:00Z", "2026-11-16T08:18:00Z"];

/// The times of the track fixture's readings.
const READING_TIMES: [&str; 3] = ["2026-11-16T08:00:00Z", "2026-11-16T08:10:00Z", "2026-11-16T08:20:00Z"];

/// Runs the track snapshot `snapshot` through the mapping that writes the bus's track as `location`
/// instances, on `store`, into series output.
///
/// Each record is validated on its own before series output gathers its instances, and the record
/// of an Observation has no `historicalLocation`, so its entity has no `location`, which the
/// `AirQualityObserved` schema requires; the run validates against a schema that accepts anything.
fn run(store: StoreKind, snapshot: &Path) -> RunOutcome {
    let (result, outcome) = attempt_file_end_to_end(
        store,
        snapshot,
        &fixture("tracked-air-quality-observed.json5"),
        &RunSchemas::Permissive(&["dataModel.Environment/AirQualityObserved"]),
        SERIES_OUTPUT,
    );
    result.unwrap();
    outcome
}

/// The value of every instance of the series attribute `attribute`, in the order written.
fn values(attribute: &Value) -> Vec<&Value> {
    attribute
        .as_array()
        .expect("a series attribute is an array of instances")
        .iter()
        .map(|instance| &instance["value"])
        .collect()
}

#[test]
fn a_thing_s_track_is_written_as_one_location_per_historical_location_beside_its_readings() {
    for store in STORES {
        let outcome = run(store, &fixture("track-v1.1-range.jsonl"));

        assert_eq!(entity_ids(&outcome), [BUS], "{store:?}");
        let bus = entity(&outcome, BUS);
        let points: Vec<Value> = TRACK.iter().map(|coordinates| json!({"type": "Point", "coordinates": coordinates})).collect();
        assert_eq!(values(&bus["location"]), points.iter().collect::<Vec<_>>(), "{store:?}");
        assert_eq!(instant_times(&bus["location"]), TRACK_TIMES, "{store:?}");
        assert_eq!(values(&bus["pm10"]), [&json!(18.2), &json!(21.5), &json!(24.9)], "{store:?}");
        assert_eq!(instant_times(&bus["pm10"]), READING_TIMES, "{store:?}");
        assert!(outcome.diagnostics.is_empty(), "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn historical_locations_that_give_no_record_are_reported_once_and_the_rest_are_written() {
    let folder = TempDir::new().unwrap();
    let snapshot = folder.path().join("track.jsonl");
    let mut text = fs::read_to_string(fixture("track-v1.1-range.jsonl")).unwrap();
    text.push_str(r#"{"historicalLocation":{"@iot.id":34,"time":"2026-11-16","Locations":[]},"thing":12}"#);
    text.push('\n');
    text.push_str(r#"{"historicalLocation":{"@iot.id":35,"time":"2026-11-16T08:28:00Z","Locations":[{"@iot.id":104,"encodingType":"text/plain","location":"Depot"}]},"thing":12}"#);
    text.push('\n');
    fs::write(&snapshot, text).unwrap();

    for store in STORES {
        let outcome = run(store, &snapshot);

        assert_eq!(instant_times(&entity(&outcome, BUS)["location"]), TRACK_TIMES, "{store:?}");
        let reported = outcome.coded(DiagnosticCode::Ingest(IngestCode::HistoricalLocationsUnreadable));
        assert_eq!(reported.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(
            reported[0].headline,
            format!(
                "In `{}`, 2 HistoricalLocations give no record: HistoricalLocation 34 of Thing 12 has the `time` `2026-11-16`, which names no instant, and 1 more",
                snapshot.display()
            ),
            "{store:?}"
        );
        assert_eq!(reported[0].occurrences, 2, "{store:?}");
        assert_eq!(outcome.warnings, 2, "{store:?}");
    }
}

/// The instant `time` on 2026-11-12, in UTC.
fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
}

/// One `HistoricalLocation` a mock server holds: its time, and the `HistoricalLocation` as served.
struct Held {
    time: DateTime<Utc>,
    historical_location: Value,
}

/// A SensorThings server serving the Datastream of the track fixture's bus, without Observations,
/// and the `HistoricalLocations` of the bus it holds when a request reaches it, as a server applies a
/// request's `time` bounds.
struct TrackServer {
    server: ServerGuard,
    held: Arc<Mutex<Vec<Held>>>,
    _mocks: Vec<Mock>,
}

impl TrackServer {
    fn new() -> TrackServer {
        let mut server = mockito::Server::new();
        let held: Arc<Mutex<Vec<Held>>> = Arc::default();
        let text = fs::read_to_string(fixture("track-v1.1-range.jsonl")).unwrap();
        let datastream = serde_json::from_str::<Value>(text.lines().nth(1).unwrap()).unwrap()["datastream"].take();
        let answering = Arc::clone(&held);
        let mocks = vec![
            server
                .mock(
                    "GET",
                    "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000",
                )
                .with_body(json!({ "value": [datastream] }).to_string())
                .create(),
            server
                .mock("GET", Matcher::Regex(r"^/v1\.1/Datastreams\(7\)/Observations\?".to_owned()))
                .with_body(json!({ "value": [] }).to_string())
                .create(),
            server
                .mock("GET", Matcher::Regex(r"^/v1\.1/Things\(12\)/HistoricalLocations\?".to_owned()))
                .with_body_from_request(move |request: &Request| {
                    let (from, to) = time_bounds(request.path_and_query());
                    let holding = answering.lock().unwrap();
                    let mut answer: Vec<&Held> = holding.iter().filter(|held| from <= held.time && held.time < to).collect();
                    answer.sort_by_key(|held| held.time);
                    json!({ "value": answer.iter().map(|held| &held.historical_location).collect::<Vec<_>>() })
                        .to_string()
                        .into_bytes()
                })
                .create(),
        ];
        TrackServer { server, held, _mocks: mocks }
    }

    /// Holds the `HistoricalLocation` `id`, at `time` on 2026-11-12 at the point `coordinates`.
    fn hold(&self, id: i64, time: &str, coordinates: [f64; 2]) {
        let historical_location = json!({
            "@iot.id": id,
            "time": format!("2026-11-12T{time}:00Z"),
            "Locations": [{"@iot.id": 100 + id, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": coordinates}}],
        });
        self.held.lock().unwrap().push(Held {
            time: at(time),
            historical_location,
        });
    }

    /// Runs the input that reads the server incrementally through the cursor `bus-12` with
    /// `HistoricalLocations`, starting the bus at 2026-11-12, once into series files, keeping its
    /// cursors in `cursors`.
    fn run(&self, cursors: &Path) -> RunOutcome {
        let declared = format!(
            r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}", sensorthings: {{ read: {{ incremental: {{ cursor: "bus-12", initial: "2026-11-12T00:00:00Z" }} }}, historicalLocations: true }} }}"#,
            self.server.url(),
            fixture("tracked-air-quality-observed.json5").display()
        );
        let schemas = RunSchemas::Permissive(&["dataModel.Environment/AirQualityObserved"]);
        let setup = RunSetup {
            cursors: Some(cursors),
            ..RunSetup::files(&schemas, SERIES_OUTPUT)
        };
        let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &declared, &setup);
        result.unwrap();
        outcome
    }

    /// The high-water mark the cursor `bus-12` in `cursors` keeps for the bus's `HistoricalLocations`.
    fn thing_high_water(&self, cursors: &Path) -> Option<DateTime<Utc>> {
        let root = ServiceRoot::try_from(Url::parse(&format!("{}/v1.1", self.server.url())).unwrap()).unwrap();
        let store = CursorStore::open(cursors, CursorName::try_from("bus-12".to_owned()).unwrap(), root).unwrap();
        store
            .positions()
            .unwrap()
            .get(&ReadStream::Thing(EntityId::Integer(12)))
            .map(Position::high_water)
    }
}

/// The `time` bounds the `HistoricalLocations` request `path_and_query` asks for.
fn time_bounds(path_and_query: &str) -> (DateTime<Utc>, DateTime<Utc>) {
    let instant = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().to_utc();
    let (_, filter) = path_and_query.split_once("$filter=time%20ge%20").unwrap();
    let (from, rest) = filter.split_once("%20and%20time%20lt%20").unwrap();
    let (to, _) = rest.split_once('&').unwrap();
    (instant(from), instant(to))
}

#[test]
fn a_second_incremental_cycle_writes_the_historical_locations_added_since_the_first_alone() {
    let server = TrackServer::new();
    let cursors = TempDir::new().unwrap();
    server.hold(31, "07:58", TRACK[0]);
    server.hold(32, "08:08", TRACK[1]);

    let first = server.run(cursors.path());

    let bus = "urn:ngsi-ld:AirQualityObserved:sta-127001-12";
    assert_eq!(
        instant_times(&entity(&first, bus)["location"]),
        ["2026-11-12T07:58:00Z", "2026-11-12T08:08:00Z"]
    );
    assert_eq!(server.thing_high_water(cursors.path()), Some(at("08:08")));

    server.hold(33, "08:18", TRACK[2]);
    server.hold(30, "07:40", TRACK[0]);
    let second = server.run(cursors.path());

    assert_eq!(
        instant_times(&entity(&second, bus)["location"]),
        ["2026-11-12T07:40:00Z", "2026-11-12T08:18:00Z"]
    );
    assert_eq!(server.thing_high_water(cursors.path()), Some(at("08:18")));
    assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
}

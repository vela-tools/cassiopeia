use crate::end_to_end::{
    harness::{RunOutcome, RunSchemas, RunSetup, STORES, attempt_file_end_to_end, attempt_scheduled_end_to_end, attempt_setup_end_to_end, entity_ids},
    incremental_reads::bounds,
    sensorthings::{SERIES_OUTPUT, entity, fixture, instant_times},
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode};
use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, read_stream::ReadStream, service_root::ServiceRoot, stream_id::StreamId};
use cassiopeia_sensorthings_cursor::{
    cursor_store::CursorStore,
    position::Position,
    positions::Positions,
    reset::{IdText, ResetScope, ResetTarget},
};
use chrono::{DateTime, Utc};
use mockito::{Matcher, Mock, Request, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use temp_dir::TempDir;
use url::Url;

/// The station of the wind fixtures, as it is written from a snapshot of `https://h.example/v1.1`.
const STATION: &str = "urn:ngsi-ld:AirQualityObserved:sta-hexample-1";

/// The station of the wind fixtures, as it is written from a read of the mock server.
const MOCK_STATION: &str = "urn:ngsi-ld:AirQualityObserved:sta-127001-1";

/// The path and query of the metadata discovery of the mock server's Datastreams.
const DATASTREAMS: &str = "/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc";

/// The path and query of the metadata discovery of the mock server's `MultiDatastreams`.
const MULTI_DATASTREAMS: &str = "/v1.1/MultiDatastreams?$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc";

/// The path and query of the latest discovery of the mock server's Datastreams.
const LATEST_DATASTREAMS: &str =
    "/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc";

/// The data stream line of kind `kind`, `datastream` or `multiDatastream`, of the wind fixture
/// `name`, without its Observations, as a server lists it.
fn listed(name: &str, kind: &str) -> Value {
    let mut line = fs::read_to_string(fixture(name))
        .unwrap()
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find_map(|mut line| line.get_mut(kind).map(Value::take))
        .unwrap();
    line.as_object_mut().unwrap().remove("Observations");
    line
}

/// The instant `time` on 2026-11-12, in UTC.
fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
}

/// Runs the wind snapshot fixture `snapshot` through the wind mapping on `store`, with
/// `output_extra` spliced into the output, validating against the fixtures' schemas.
fn run_snapshot(store: StoreKind, snapshot: &str, output_extra: &str) -> RunOutcome {
    let (result, outcome) = attempt_file_end_to_end(
        store,
        &fixture(snapshot),
        &fixture("wind-air-quality-observed.json5"),
        &RunSchemas::Folder(&fixture("schemas")),
        output_extra,
    );
    result.unwrap();
    outcome
}

#[test]
fn a_latest_snapshot_maps_the_elements_of_a_multi_datastream_beside_the_datastreams() {
    for store in STORES {
        let outcome = run_snapshot(store, "wind-v1.1-latest.jsonl", r#", validation: { mode: "fail" }"#);

        assert_eq!(entity_ids(&outcome), [STATION], "{store:?}");
        let station = entity(&outcome, STATION);
        assert_eq!(station["pm10"]["value"], 18.2, "{store:?}");
        assert_eq!(station["windSpeed"]["value"], 4.2, "{store:?}");
        assert_eq!(station["windSpeed"]["unitCode"], "MTS", "{store:?}");
        assert_eq!(station["windDirection"]["value"], 135.0, "{store:?}");
        assert_eq!(station["windDirection"]["unitCode"], "DD", "{store:?}");
        for attribute in ["windSpeed", "windDirection"] {
            assert_eq!(station[attribute]["observedAt"], "2026-11-13T07:00:00Z", "{store:?} {attribute}");
        }
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
    }
}

#[test]
fn a_range_snapshot_of_a_multi_datastream_writes_every_observation_of_every_element() {
    for store in STORES {
        let outcome = run_snapshot(store, "wind-v1.1-range.jsonl", SERIES_OUTPUT);

        let station = entity(&outcome, STATION);
        let times = ["2026-11-13T05:00:00Z", "2026-11-13T06:00:00Z", "2026-11-13T07:00:00Z"];
        assert_eq!(instant_times(&station["windSpeed"]), times, "{store:?}");
        assert_eq!(instant_times(&station["windDirection"]), times, "{store:?}");
        let directions: Vec<&Value> = station["windDirection"].as_array().unwrap().iter().map(|instance| &instance["value"]).collect();
        assert_eq!(directions, [&json!(120.0), &json!(135.0), &json!(150.0)], "{store:?}");
    }
}

/// An Observation a mock server holds: the collection of its data stream, the first cycle that sees
/// it, its time, and the Observation as served.
struct Held {
    collection: &'static str,
    from_cycle: usize,
    time: DateTime<Utc>,
    observation: Value,
}

/// A SensorThings server holding Datastream 4, `pm10`, and `MultiDatastream` 4, wind speed and
/// direction, both of Thing 1 of the wind fixtures, and the Observations each holds from a cycle on,
/// answering each request as a server applies its `phenomenonTime` bounds.
///
/// Every discovery of its Datastreams starts a cycle.
struct WindServer {
    server: ServerGuard,
    held: Arc<Mutex<Vec<Held>>>,
    answered: Arc<Mutex<Vec<String>>>,
    _mocks: Vec<Mock>,
}

impl WindServer {
    fn new() -> WindServer {
        let mut server = mockito::Server::new();
        let held: Arc<Mutex<Vec<Held>>> = Arc::default();
        let cycles: Arc<AtomicUsize> = Arc::default();
        let answered: Arc<Mutex<Vec<String>>> = Arc::default();
        let mut pm10 = listed("wind-v1.1-latest.jsonl", "datastream");
        pm10["@iot.id"] = json!(4);
        let wind = listed("wind-v1.1-latest.jsonl", "multiDatastream");
        let counting = Arc::clone(&cycles);
        let mut mocks = vec![
            server
                .mock("GET", DATASTREAMS)
                .with_body_from_request(move |_| {
                    counting.fetch_add(1, Ordering::SeqCst);
                    json!({ "value": [&pm10] }).to_string().into_bytes()
                })
                .create(),
            server.mock("GET", MULTI_DATASTREAMS).with_body(json!({ "value": [wind] }).to_string()).create(),
        ];
        for collection in ["Datastreams", "MultiDatastreams"] {
            let (held, cycles, answered) = (Arc::clone(&held), Arc::clone(&cycles), Arc::clone(&answered));
            mocks.push(
                server
                    .mock("GET", Matcher::Regex(format!(r"^/v1\.1/{collection}\(4\)/Observations\?")))
                    .with_body_from_request(move |request: &Request| {
                        let path = request.path_and_query().to_owned();
                        let (from, to) = bounds(&path);
                        answered.lock().unwrap().push(path);
                        let cycle = cycles.load(Ordering::SeqCst);
                        let mut answer: Vec<(DateTime<Utc>, Value)> = held
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|held| held.collection == collection && held.from_cycle <= cycle && from <= held.time && held.time < to)
                            .map(|held| (held.time, held.observation.clone()))
                            .collect();
                        answer.sort_by_key(|(time, _)| *time);
                        json!({ "value": answer.into_iter().map(|(_, observation)| observation).collect::<Vec<_>>() })
                            .to_string()
                            .into_bytes()
                    })
                    .create(),
            );
        }
        WindServer {
            server,
            held,
            answered,
            _mocks: mocks,
        }
    }

    /// Holds the Observation `id` of the data stream 4 of `collection`, made at `time` on 2026-11-12
    /// with the result `result`, from the cycle `from_cycle` on.
    fn hold(&self, collection: &'static str, from_cycle: usize, id: i64, time: &str, result: &Value) {
        self.held.lock().unwrap().push(Held {
            collection,
            from_cycle,
            time: at(time),
            observation: json!({"@iot.id": id, "phenomenonTime": format!("2026-11-12T{time}:00Z"), "result": result}),
        });
    }

    /// The service root, `{server}/v1.1`.
    fn root(&self) -> ServiceRoot {
        ServiceRoot::try_from(Url::parse(&format!("{}/v1.1", self.server.url())).unwrap()).unwrap()
    }

    /// The input that reads the server's Datastreams and `MultiDatastreams` incrementally through the
    /// cursor `wind`, starting each data stream without a position at 2026-01-01.
    fn declared(&self) -> String {
        format!(
            r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}", sensorthings: {{ multiDatastreams: true, read: {{ incremental: {{ cursor: "wind", initial: "2026-01-01T00:00:00Z" }} }} }} }}"#,
            self.server.url(),
            fixture("wind-air-quality-observed.json5").display()
        )
    }

    /// How many Observations requests the server answered for the data stream 4 of `collection`.
    fn observation_reads(&self, collection: &str) -> usize {
        let key = format!("/v1.1/{collection}(4)/Observations?");
        self.answered.lock().unwrap().iter().filter(|request| request.starts_with(&key)).count()
    }
}

/// Runs the incremental input of `server` once into series files, keeping its cursors in `cursors`.
fn run_incremental(server: &WindServer, cursors: &Path) -> RunOutcome {
    let folder = fixture("schemas");
    let schemas = RunSchemas::Folder(&folder);
    let setup = RunSetup {
        cursors: Some(cursors),
        ..RunSetup::files(&schemas, SERIES_OUTPUT)
    };
    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &server.declared(), &setup);
    result.unwrap();
    outcome
}

/// The cursor `wind` of `server` in `cursors`, once no run holds it.
fn store(server: &WindServer, cursors: &Path) -> CursorStore {
    CursorStore::open(cursors, CursorName::try_from("wind".to_owned()).unwrap(), server.root()).unwrap()
}

/// Each position of `positions` as its data stream and high-water mark, as `cursor show` lists them.
fn high_waters(positions: &Positions) -> Vec<(String, DateTime<Utc>)> {
    positions
        .iter()
        .map(|(datastream, position)| (datastream.to_string(), Position::high_water(position)))
        .collect()
}

#[test]
fn incremental_cycles_keep_a_position_for_a_datastream_and_a_multi_datastream_of_one_id() {
    let server = WindServer::new();
    server.hold("Datastreams", 1, 1, "05:00", &json!(18.2));
    server.hold("MultiDatastreams", 1, 2, "05:30", &json!([3.9, 120]));
    server.hold("MultiDatastreams", 1, 3, "06:00", &json!([4.2, 135]));
    server.hold("Datastreams", 2, 4, "08:00", &json!(19.0));
    server.hold("MultiDatastreams", 2, 5, "08:30", &json!([5.1, 150]));
    let cursors = TempDir::new().unwrap();

    let first = run_incremental(&server, cursors.path());
    let after_first = store(&server, cursors.path()).positions().unwrap();
    let second = run_incremental(&server, cursors.path());
    let after_second = store(&server, cursors.path()).positions().unwrap();

    assert_eq!(
        instant_times(&entity(&first, MOCK_STATION)["windSpeed"]),
        ["2026-11-12T05:30:00Z", "2026-11-12T06:00:00Z"]
    );
    assert_eq!(
        high_waters(&after_first),
        [("Datastream 4".to_owned(), at("05:00")), ("MultiDatastream 4".to_owned(), at("06:00"))]
    );
    assert_eq!(instant_times(&entity(&second, MOCK_STATION)["windSpeed"]), ["2026-11-12T08:30:00Z"]);
    assert_eq!(instant_times(&entity(&second, MOCK_STATION)["pm10"]), ["2026-11-12T08:00:00Z"]);
    assert_eq!(
        high_waters(&after_second),
        [("Datastream 4".to_owned(), at("08:00")), ("MultiDatastream 4".to_owned(), at("08:30"))]
    );
    assert_eq!((server.observation_reads("Datastreams"), server.observation_reads("MultiDatastreams")), (2, 2));
}

#[test]
fn a_reset_of_the_multi_datastream_leaves_the_datastream_of_its_id_where_it_was() {
    let server = WindServer::new();
    server.hold("Datastreams", 1, 1, "05:00", &json!(18.2));
    server.hold("MultiDatastreams", 1, 2, "06:00", &json!([4.2, 135]));
    let cursors = TempDir::new().unwrap();
    run_incremental(&server, cursors.path());
    let store = store(&server, cursors.path());
    let before = store.positions().unwrap();

    store
        .reset(&ResetScope::MultiDatastream(IdText::from("4".to_owned())), ResetTarget::InitialPoint)
        .unwrap();
    let after = store.positions().unwrap();

    let datastream = ReadStream::Data(StreamId::Datastream(EntityId::Integer(4)));
    assert_eq!(high_waters(&after), [("Datastream 4".to_owned(), at("05:00"))]);
    assert_eq!(after.get(&datastream), before.get(&datastream));
}

#[test]
fn a_run_that_reads_no_multi_datastream_reports_those_its_server_holds_once() {
    let mut server = mockito::Server::new();
    let mut pm10 = listed("wind-v1.1-latest.jsonl", "datastream");
    pm10["Observations"] = json!([{"@iot.id": 1101, "phenomenonTime": "2026-11-13T07:00:00Z", "result": 18.2}]);
    let mocks = [
        server
            .mock("GET", LATEST_DATASTREAMS)
            .with_body(json!({ "value": [pm10] }).to_string())
            .expect(2)
            .create(),
        server
            .mock("GET", "/v1.1")
            .with_body(json!({"value": [{"name": "Datastreams"}, {"name": "MultiDatastreams"}]}).to_string())
            .expect(1)
            .create(),
        server
            .mock("GET", "/v1.1/MultiDatastreams?$top=1")
            .with_body(json!({"value": [{"@iot.id": 4}]}).to_string())
            .expect(1)
            .create(),
    ];
    let declared = format!(
        r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}" }}"#,
        server.url(),
        fixture("wind-air-quality-observed.json5").display()
    );

    // Two cycles two seconds apart, as a cycle start is held to the second.
    let (result, outcome) = attempt_scheduled_end_to_end(
        StoreKind::DashMap,
        &declared,
        &RunSchemas::Folder(&fixture("schemas")),
        "",
        r#"{ mode: "every", value: "2s", repeat: 2 }"#,
    );

    result.unwrap();
    mocks.iter().for_each(Mock::assert);
    let unread = outcome.coded(DiagnosticCode::Collect(CollectCode::MultiDatastreamsUnread));
    assert_eq!(unread.len(), 1, "{:?}", outcome.diagnostics);
    assert_eq!(
        unread[0].headline,
        format!(
            "`{}/v1.1` holds MultiDatastreams, which this input reads none of; set `multiDatastreams` to `true`, or to an OData expression that selects some, to read their readings",
            server.url()
        )
    );
    assert_eq!(entity(&outcome, MOCK_STATION)["pm10"]["value"], 18.2);
}

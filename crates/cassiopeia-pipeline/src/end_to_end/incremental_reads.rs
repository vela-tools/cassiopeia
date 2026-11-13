use crate::{
    end_to_end::{
        harness::{RunOutcome, RunOutput, RunSchemas, RunSetup, attempt_setup_end_to_end},
        sensorthings::{MOCK_RANGE_THING, SERIES_OUTPUT, entity, fixture, instant_times},
    },
    error::PipelineError,
    test_engine::SHUTDOWN,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};
use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, service_root::ServiceRoot};
use cassiopeia_sensorthings_cursor::{
    cursor_store::CursorStore,
    cursor_store_error::CursorStoreError,
    position::Position,
    positions::Positions,
    reset::{ResetScope, ResetTarget},
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use temp_dir::TempDir;
use url::Url;

/// The RIVM data stream of the fixture Thing's PM10 readings.
const PM10: i64 = 56776;

/// The RIVM data stream of the fixture Thing's PM2.5 readings.
const PM25: i64 = 56775;

/// The path and query of the metadata discovery request of the mock server's `/v1.0` root.
const METADATA: &str = "/v1.0/Datastreams?$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=1000";

/// The flag the cancellation test raises once the server answered the last request of collection.
static STOPPED_AFTER_COLLECTION: AtomicBool = AtomicBool::new(false);

/// The instant `time` on 2026-11-12, in UTC.
fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
}

/// One Observation a mock server holds: its data stream, the first cycle that sees it, and the
/// Observation as served.
struct Held {
    datastream: i64,
    from_cycle: usize,
    time: DateTime<Utc>,
    observation: Value,
}

/// A SensorThings server serving the two RIVM data streams of the range fixture, and the
/// Observations each holds when a request reaches it, as a server applies a request's
/// `phenomenonTime` bounds.
///
/// Every metadata discovery starts a cycle, so an Observation held from cycle 2 on appears to the
/// second cycle, or the second run, that reads the server.
struct RivmServer {
    server: ServerGuard,
    held: Arc<Mutex<Vec<Held>>>,
    cycles: Arc<AtomicUsize>,
    answered: Arc<Mutex<Vec<String>>>,
    _mocks: Vec<Mock>,
}

impl RivmServer {
    /// The server, raising `stop` once it has answered a request for the Observations of PM10, the
    /// last data stream a cycle reads, when given.
    fn new(stop: Option<&'static AtomicBool>) -> RivmServer {
        let mut server = mockito::Server::new();
        let held: Arc<Mutex<Vec<Held>>> = Arc::default();
        let cycles: Arc<AtomicUsize> = Arc::default();
        let answered: Arc<Mutex<Vec<String>>> = Arc::default();
        let datastreams: Vec<Value> = fs::read_to_string(fixture("rivm-v1.0-range.jsonl"))
            .unwrap()
            .lines()
            .skip(1)
            .filter_map(|line| serde_json::from_str::<Value>(line).unwrap().get_mut("datastream").map(Value::take))
            .filter(Value::is_object)
            .collect();
        let page = json!({ "value": datastreams }).to_string();
        let discovered = {
            let (cycles, answered) = (Arc::clone(&cycles), Arc::clone(&answered));
            server
                .mock("GET", METADATA)
                .with_body_from_request(move |request: &Request| {
                    cycles.fetch_add(1, Ordering::SeqCst);
                    answered.lock().unwrap().push(request.path_and_query().to_owned());
                    page.clone().into_bytes()
                })
                .create()
        };
        let mut mocks = vec![discovered];
        for datastream in [PM25, PM10] {
            let (held, cycles, answered) = (Arc::clone(&held), Arc::clone(&cycles), Arc::clone(&answered));
            mocks.push(
                server
                    .mock("GET", Matcher::Regex(format!(r"^/v1\.0/Datastreams\({datastream}\)/Observations\?")))
                    .with_body_from_request(move |request: &Request| {
                        let path = request.path_and_query().to_owned();
                        let (from, to) = bounds(&path);
                        answered.lock().unwrap().push(path);
                        let cycle = cycles.load(Ordering::SeqCst);
                        let mut answer: Vec<(DateTime<Utc>, Value)> = held
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|held| held.datastream == datastream && held.from_cycle <= cycle && from <= held.time && held.time < to)
                            .map(|held| (held.time, held.observation.clone()))
                            .collect();
                        answer.sort_by_key(|(time, _)| *time);
                        if datastream == PM10
                            && let Some(stop) = stop
                        {
                            stop.store(true, Ordering::SeqCst);
                        }
                        json!({ "value": answer.into_iter().map(|(_, observation)| observation).collect::<Vec<_>>() })
                            .to_string()
                            .into_bytes()
                    })
                    .create(),
            );
        }
        RivmServer {
            server,
            held,
            cycles,
            answered,
            _mocks: mocks,
        }
    }

    /// Holds the Observation `id` of the data stream `datastream`, made at `time` on 2026-11-12 with
    /// the result `result`, from the cycle `from_cycle` on.
    fn hold(&self, datastream: i64, from_cycle: usize, id: i64, time: &str, result: f64) {
        self.held.lock().unwrap().push(Held {
            datastream,
            from_cycle,
            time: at(time),
            observation: json!({"@iot.id": id, "phenomenonTime": format!("2026-11-12T{time}:00.000Z"), "result": result}),
        });
    }

    /// The service root, `{server}/v1.0`.
    fn root(&self) -> ServiceRoot {
        ServiceRoot::try_from(Url::parse(&format!("{}/v1.0", self.server.url())).unwrap()).unwrap()
    }

    /// The input that reads the server incrementally through the cursor `rivm-air`, starting each
    /// data stream without a position at 2026-01-01.
    fn declared(&self) -> String {
        format!(
            r#"{{ source: "{}/v1.0", format: "sensorthings", mapping: "{}", sensorthings: {{ read: {{ incremental: {{ cursor: "rivm-air", initial: "2026-01-01T00:00:00Z" }} }} }} }}"#,
            self.server.url(),
            fixture("air-quality-observed.json5").display()
        )
    }

    /// The `phenomenonTime` lower bound of every request for the Observations of `datastream`, in
    /// the order answered.
    fn lower_bounds(&self, datastream: i64) -> Vec<DateTime<Utc>> {
        let key = format!("/Datastreams({datastream})/Observations?");
        self.answered
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.contains(&key))
            .map(|request| bounds(request).0)
            .collect()
    }

    /// How many requests the server answered.
    fn answered(&self) -> usize {
        self.answered.lock().unwrap().len()
    }

    /// How many cycles have read the server.
    fn cycles(&self) -> usize {
        self.cycles.load(Ordering::SeqCst)
    }
}

/// The `phenomenonTime` bounds the observations request `path_and_query` asks for.
fn bounds(path_and_query: &str) -> (DateTime<Utc>, DateTime<Utc>) {
    let instant = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().to_utc();
    let (_, filter) = path_and_query.split_once("phenomenonTime%20ge%20").unwrap();
    let (from, rest) = filter.split_once("%20and%20phenomenonTime%20lt%20").unwrap();
    let (to, _) = rest.split_once('&').unwrap();
    (instant(from), instant(to))
}

/// Runs the incremental input of `server` once into series files, keeping its cursors in `cursors`.
fn run_into_files(server: &RivmServer, cursors: &Path) -> (Result<(), PipelineError>, RunOutcome) {
    let folder = fixture("schemas");
    let schemas = RunSchemas::Folder(&folder);
    let setup = RunSetup {
        cursors: Some(cursors),
        ..RunSetup::files(&schemas, SERIES_OUTPUT)
    };
    attempt_setup_end_to_end(StoreKind::DashMap, &server.declared(), &setup)
}

/// Runs the incremental input of `server` once into the broker's temporal API at `broker`, with
/// `atomicity`, validating with `validation_mode` against the schemas of `schemas`, keeping its
/// cursors in `cursors`.
fn run_into_broker(
    server: &RivmServer,
    cursors: &Path,
    broker: &ServerGuard,
    atomicity: &str,
    validation_mode: &str,
    schemas: &Path,
) -> (Result<(), PipelineError>, RunOutcome) {
    let schemas = RunSchemas::Folder(schemas);
    let output = format!(
        r#"{{ target: "context-broker", url: "{}/", operation: "temporal", atomicity: "{atomicity}", temporal: {{ representation: "series" }}, validation: {{ mode: "{validation_mode}" }} }}"#,
        broker.url()
    );
    let setup = RunSetup {
        schemas: &schemas,
        output: RunOutput::Declared(&output),
        schedule: None,
        cursors: Some(cursors),
        shutdown: &SHUTDOWN,
    };
    attempt_setup_end_to_end(StoreKind::DashMap, &server.declared(), &setup)
}

/// The positions the cursor `rivm-air` of `server` keeps in `cursors`, once no run holds it.
fn committed(server: &RivmServer, cursors: &Path) -> Positions {
    let name = CursorName::try_from("rivm-air".to_owned()).unwrap();
    CursorStore::open(cursors, name, server.root()).unwrap().positions().unwrap()
}

/// Resets every position of the cursor `rivm-air` in `cursors` to `target`, as
/// `cassiopeia sensorthings cursor reset` does once no run holds it.
fn reset(cursors: &Path, target: ResetTarget) {
    let name = CursorName::try_from("rivm-air".to_owned()).unwrap();
    CursorStore::open_existing(cursors, name).unwrap().reset(&ResetScope::Store, target).unwrap();
}

/// The high-water mark of the PM10 data stream among `positions`, if it has one.
fn pm10_high_water(positions: &Positions) -> Option<DateTime<Utc>> {
    positions.get(&EntityId::Integer(PM10)).map(Position::high_water)
}

/// The `observedAt` of every PM10 reading of the fixture Thing a run wrote into files.
fn written_pm10(outcome: &RunOutcome) -> Vec<String> {
    instant_times(&entity(outcome, MOCK_RANGE_THING)["pm10"])
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// A broker whose temporal API answers every request with `status`, keeping each request's body.
fn broker(status: usize) -> (ServerGuard, Arc<Mutex<Vec<Value>>>, Mock) {
    let mut broker = mockito::Server::new();
    let bodies: Arc<Mutex<Vec<Value>>> = Arc::default();
    let kept = Arc::clone(&bodies);
    let mock = broker
        .mock("POST", Matcher::Regex(r"^/ngsi-ld/v1/temporal/entities".to_owned()))
        .with_status(status)
        .with_body_from_request(move |request: &Request| {
            kept.lock().unwrap().push(serde_json::from_slice(request.body().unwrap()).unwrap());
            Vec::new()
        })
        .create();
    (broker, bodies, mock)
}

/// The `observedAt` of every PM10 instance of every entity `bodies` sent.
fn sent_pm10(bodies: &Mutex<Vec<Value>>) -> Vec<String> {
    bodies
        .lock()
        .unwrap()
        .iter()
        .flat_map(|body| body["pm10"].as_array().cloned().unwrap_or_default())
        .map(|instance| instance["observedAt"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn consecutive_cycles_write_each_reading_once() {
    let server = RivmServer::new(None);
    for (id, time) in [(1, "04:00"), (2, "05:00"), (3, "06:00")] {
        server.hold(PM10, 1, id, time, 30.0);
    }
    for (id, time) in [(4, "07:00"), (5, "08:00"), (6, "09:00")] {
        server.hold(PM10, 2, id, time, 31.0);
    }
    let cursors = TempDir::new().unwrap();
    let folder = fixture("schemas");
    let schemas = RunSchemas::Folder(&folder);
    let setup = RunSetup {
        schedule: Some(r#"{ mode: "every", value: "2s", repeat: 2 }"#),
        cursors: Some(cursors.path()),
        ..RunSetup::files(&schemas, SERIES_OUTPUT)
    };

    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &server.declared(), &setup);

    result.unwrap();
    assert_eq!(server.cycles(), 2);
    assert_eq!(server.lower_bounds(PM10)[1], at("05:00"));
    assert_eq!(written_pm10(&outcome), ["2026-11-12T07:00:00Z", "2026-11-12T08:00:00Z", "2026-11-12T09:00:00Z"]);
    assert_eq!(pm10_high_water(&committed(&server, cursors.path())), Some(at("09:00")));
}

#[test]
fn a_failed_cycle_does_not_advance_the_cursor() {
    let server = RivmServer::new(None);
    server.hold(PM10, 1, 1, "06:00", 30.0);
    server.hold(PM10, 2, 2, "07:00", 31.0);
    let cursors = TempDir::new().unwrap();
    let (failing, _, _failing_mock) = broker(500);

    run_into_files(&server, cursors.path()).0.unwrap();
    let after_success = committed(&server, cursors.path());
    let (result, _) = run_into_broker(&server, cursors.path(), &failing, "streaming", "warn", &fixture("schemas"));
    let after_failure = committed(&server, cursors.path());
    let (resumed, outcome) = run_into_files(&server, cursors.path());

    assert!(result.is_err());
    assert_eq!(after_failure, after_success);
    assert_eq!(pm10_high_water(&after_failure), Some(at("06:00")));
    resumed.unwrap();
    assert_eq!(server.lower_bounds(PM10)[1..], [at("05:00"), at("05:00")]);
    assert_eq!(written_pm10(&outcome), ["2026-11-12T07:00:00Z"]);
}

#[test]
fn a_cancelled_cycle_does_not_advance_the_cursor() {
    let server = RivmServer::new(Some(&STOPPED_AFTER_COLLECTION));
    server.hold(PM10, 1, 1, "06:00", 30.0);
    let cursors = TempDir::new().unwrap();
    let folder = fixture("schemas");
    let schemas = RunSchemas::Folder(&folder);
    let setup = RunSetup {
        cursors: Some(cursors.path()),
        shutdown: &STOPPED_AFTER_COLLECTION,
        ..RunSetup::files(&schemas, SERIES_OUTPUT)
    };

    let (result, _) = attempt_setup_end_to_end(StoreKind::DashMap, &server.declared(), &setup);

    result.unwrap();
    assert!(STOPPED_AFTER_COLLECTION.load(Ordering::SeqCst));
    assert_eq!(server.lower_bounds(PM10).len(), 1);
    assert_eq!(committed(&server, cursors.path()), Positions::default());
}

#[test]
fn an_atomic_cycle_that_fails_sends_nothing_and_the_next_sends_once() {
    let server = RivmServer::new(None);
    for (id, time) in [(1, "04:00"), (2, "05:00"), (3, "06:00")] {
        server.hold(PM10, 1, id, time, 30.0);
    }
    let cursors = TempDir::new().unwrap();
    let rejecting = TempDir::new().unwrap();
    let rejecting_schema = rejecting.path().join("dataModel.Environment");
    fs::create_dir_all(&rejecting_schema).unwrap();
    fs::write(
        rejecting_schema.join("AirQualityObserved.json"),
        r#"{"type": "object", "required": ["neverWritten"]}"#,
    )
    .unwrap();
    let (accepting, bodies, _accepting_mock) = broker(201);

    let (failed, _) = run_into_broker(&server, cursors.path(), &accepting, "atomic", "fail", rejecting.path());
    let sent_by_failure = sent_pm10(&bodies);
    let after_failure = committed(&server, cursors.path());
    let (delivered, _) = run_into_broker(&server, cursors.path(), &accepting, "atomic", "fail", &fixture("schemas"));

    assert!(failed.is_err());
    assert_eq!(sent_by_failure, Vec::<String>::new());
    assert_eq!(after_failure, Positions::default());
    delivered.unwrap();
    assert_eq!(sent_pm10(&bodies), ["2026-11-12T04:00:00Z", "2026-11-12T05:00:00Z", "2026-11-12T06:00:00Z"]);
    assert_eq!(pm10_high_water(&committed(&server, cursors.path())), Some(at("06:00")));
}

#[test]
fn a_late_upload_within_the_lookback_is_written_once_and_beyond_it_never() {
    let server = RivmServer::new(None);
    server.hold(PM10, 1, 1, "10:00", 30.0);
    server.hold(PM10, 2, 2, "09:30", 31.0);
    server.hold(PM10, 2, 3, "08:00", 32.0);
    let cursors = TempDir::new().unwrap();

    let (first, first_outcome) = run_into_files(&server, cursors.path());
    let (second, second_outcome) = run_into_files(&server, cursors.path());
    let (third, third_outcome) = run_into_files(&server, cursors.path());

    first.unwrap();
    second.unwrap();
    third.unwrap();
    assert_eq!(written_pm10(&first_outcome), ["2026-11-12T10:00:00Z"]);
    assert_eq!(written_pm10(&second_outcome), ["2026-11-12T09:30:00Z"]);
    assert!(third_outcome.entities.is_empty(), "{:?}", third_outcome.entities);
    assert_eq!(server.lower_bounds(PM10)[1..], [at("09:00"), at("09:00")]);
}

#[test]
fn a_reset_cursor_reads_again_from_the_chosen_point() {
    let server = RivmServer::new(None);
    for (id, time) in [(1, "06:00"), (2, "07:00"), (3, "08:00")] {
        server.hold(PM10, 1, id, time, 30.0);
    }
    let cursors = TempDir::new().unwrap();
    let initial = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().to_utc();

    let (first, _) = run_into_files(&server, cursors.path());
    reset(cursors.path(), ResetTarget::HighWater(at("07:30")));
    let (rewound, rewound_outcome) = run_into_files(&server, cursors.path());
    reset(cursors.path(), ResetTarget::InitialPoint);
    let (restarted, restarted_outcome) = run_into_files(&server, cursors.path());

    first.unwrap();
    rewound.unwrap();
    restarted.unwrap();
    assert_eq!(server.lower_bounds(PM10), [initial, at("06:30"), initial]);
    assert_eq!(written_pm10(&rewound_outcome), ["2026-11-12T07:00:00Z", "2026-11-12T08:00:00Z"]);
    assert_eq!(
        written_pm10(&restarted_outcome),
        ["2026-11-12T06:00:00Z", "2026-11-12T07:00:00Z", "2026-11-12T08:00:00Z"]
    );
}

#[test]
fn a_cursor_in_use_fails_the_run_before_any_request() {
    let server = RivmServer::new(None);
    let cursors = TempDir::new().unwrap();
    let name = CursorName::try_from("rivm-air".to_owned()).unwrap();
    let held = CursorStore::open(cursors.path(), name, server.root()).unwrap();

    let (result, outcome) = run_into_files(&server, cursors.path());

    let error = result.unwrap_err();
    assert!(
        matches!(&error, PipelineError::Cursor(CursorStoreError::InUse { cursor }) if cursor.as_str() == "rivm-air"),
        "{error:?}"
    );
    assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Run(RunCode::CursorUnusable));
    assert_eq!(error.to_string(), "cursor 'rivm-air' is in use by a running job");
    assert_eq!(server.answered(), 0);
    assert_eq!(outcome.entities, Vec::<Value>::new());
    drop(held);
}

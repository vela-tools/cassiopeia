//! A cycle the run's stop cut short is reported as stopped, never as completed, and a stop is no
//! error: the summary counts none, and the run ends with exit status 0. Whatever the cycle downloaded
//! before the stop goes with its scratch folder.

use crate::{
    end_to_end::harness::{RunOutcome, RunSchemas, RunSetup, RunStop, attempt_setup_end_to_end},
    error::PipelineError,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};
use cassiopeia_mapping::error::MappingError;
use mockito::{Matcher, Mock, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use temp_dir::TempDir;

/// The flag the scheduled SensorThings test raises while the server answers the first page.
static STOPPED_BETWEEN_PAGES: AtomicBool = AtomicBool::new(false);

/// The flag the one-shot SensorThings test raises while the server answers the first page.
static ONE_SHOT_STOPPED_BETWEEN_PAGES: AtomicBool = AtomicBool::new(false);

/// The flag the CSV test raises while the server answers the download.
static STOPPED_DURING_DOWNLOAD: AtomicBool = AtomicBool::new(false);

/// The flag the harness raises once the second cycle of the between-cycles test has ended.
static STOPPED_AFTER_SECOND_CYCLE: AtomicBool = AtomicBool::new(false);

/// A schedule that never ends by itself, so only a stop ends the run.
const EVERY_MINUTE: &str = r#"{ mode: "every", value: "1m" }"#;

/// The CSV document every CSV input reads: two stations.
const STATIONS: &str = "id,name,temperature\nS1,Centrum,12.5\nS2,Haven,11.0\n";

/// A mapping of the CSV stations into `Station` entities.
const STATION_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ id }}" },
    attributes: {
        name: { source: "{{ name }}" },
        temperature: { source: "{{ temperature }}", transformation: "float" },
    },
}"#;

/// A mapping of each SensorThings Thing into a `Station` entity.
const THING_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ thing.id }}" },
    attributes: { name: { source: "{{ thing.name }}" } },
}"#;

/// The mapping documents of one test, in a folder of their own.
struct Mappings {
    folder: TempDir,
}

impl Mappings {
    /// An empty folder.
    fn new() -> Mappings {
        Mappings {
            folder: TempDir::new().unwrap(),
        }
    }

    /// Writes `document` as the mapping `name` and returns its path.
    fn write(&self, name: &str, document: &str) -> PathBuf {
        let path = self.folder.path().join(name);
        fs::write(&path, document).unwrap();
        path
    }
}

/// A SensorThings server whose Datastreams come one to a page, over two pages, that raises `stop`
/// while it answers the first, so a stop lands between the two pages.
struct TwoPageServer {
    server: ServerGuard,
    _first: Mock,
    second: Mock,
}

impl TwoPageServer {
    /// The server, raising `stop` while it answers the first page.
    fn new(stop: &'static AtomicBool) -> TwoPageServer {
        let mut server = mockito::Server::new();
        let first_page = json!({
            "value": [datastream(1, "A", 12.5)],
            "@iot.nextLink": format!("{}/v1.1/page2", server.url()),
        })
        .to_string();
        let first = server
            .mock("GET", Matcher::Regex(r"^/v1\.1/Datastreams\?".to_owned()))
            .with_body_from_request(move |_| {
                stop.store(true, Ordering::SeqCst);
                first_page.clone().into_bytes()
            })
            .expect(1)
            .create();
        let second = server
            .mock("GET", "/v1.1/page2")
            .with_body(json!({ "value": [datastream(2, "B", 11.0)] }).to_string())
            .expect(0)
            .create();
        TwoPageServer { server, _first: first, second }
    }

    /// The input that reads the server's newest readings, one Datastream to a page, mapped by
    /// `mapping`.
    fn declared(&self, mapping: &Path) -> String {
        format!(
            r#"{{ source: "{}/v1.1", format: "sensorthings", mapping: "{}", sensorthings: {{ pageSize: 1 }} }}"#,
            self.server.url(),
            mapping.display()
        )
    }
}

/// The Datastream `id` of the Thing `thing`, with one PM10 reading of `result`.
fn datastream(id: u64, thing: &str, result: f64) -> Value {
    json!({
        "@iot.id": id,
        "name": format!("{thing}-pm10"),
        "unitOfMeasurement": { "symbol": "ug/m3" },
        "Thing": { "@iot.id": id, "name": thing, "Locations": [] },
        "ObservedProperty": { "@iot.id": 1, "name": "pm10" },
        "Observations": [{ "@iot.id": id, "phenomenonTime": "2026-11-13T10:00:00Z", "result": result }],
    })
}

/// The line of every cycle the run reported, without its time and duration, such as
/// `#00001 stopped`.
fn cycle_lines(outcome: &RunOutcome) -> Vec<String> {
    outcome
        .messages
        .iter()
        .filter_map(|message| {
            let (_, line) = message.strip_prefix('[')?.split_once("] ")?;
            let (line, _) = line.rsplit_once(" (")?;
            Some(line.to_owned())
        })
        .collect()
}

/// Whether the run ended its schedule, which it says last.
fn finished_its_schedule(outcome: &RunOutcome) -> bool {
    outcome.messages.last().is_some_and(|message| message.starts_with("Schedule finished"))
}

#[test]
fn a_scheduled_cycle_stopped_between_two_pages_is_reported_stopped_and_counts_no_error() {
    let server = TwoPageServer::new(&STOPPED_BETWEEN_PAGES);
    let mappings = Mappings::new();
    let declared = server.declared(&mappings.write("station.json5", THING_MAPPING));
    let schemas = RunSchemas::Permissive(&["Station"]);
    let setup = RunSetup {
        schedule: Some(EVERY_MINUTE),
        stop: RunStop::raised_by(&STOPPED_BETWEEN_PAGES),
        ..RunSetup::files(&schemas, "")
    };

    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &declared, &setup);

    result.unwrap();
    server.second.assert();
    assert_eq!(cycle_lines(&outcome), ["#00001 stopped"]);
    assert_eq!(outcome.errors, 0);
    assert!(outcome.coded(DiagnosticCode::Run(RunCode::CycleFailed)).is_empty());
    assert_eq!(outcome.entities, Vec::<Value>::new());
    assert!(finished_its_schedule(&outcome), "{:?}", outcome.messages);
    assert_eq!(outcome.temporary_entries_after_cycles, [Vec::<PathBuf>::new()]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_scheduled_csv_cycle_stopped_while_its_download_is_in_flight_is_reported_stopped() {
    let mut server = mockito::Server::new();
    let _download = server
        .mock("GET", "/stations.csv")
        .with_body_from_request(|_| {
            STOPPED_DURING_DOWNLOAD.store(true, Ordering::SeqCst);
            STATIONS.as_bytes().to_vec()
        })
        .expect(1)
        .create();
    let mappings = Mappings::new();
    let declared = format!(
        r#"{{ source: "{}/stations.csv", format: "csv", mapping: "{}" }}"#,
        server.url(),
        mappings.write("station.json5", STATION_MAPPING).display()
    );
    let schemas = RunSchemas::Permissive(&["Station"]);
    let setup = RunSetup {
        schedule: Some(EVERY_MINUTE),
        stop: RunStop::raised_by(&STOPPED_DURING_DOWNLOAD),
        ..RunSetup::files(&schemas, "")
    };

    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &declared, &setup);

    result.unwrap();
    assert_eq!(cycle_lines(&outcome), ["#00001 stopped"]);
    assert_eq!(outcome.errors, 0);
    assert_eq!(outcome.entities, Vec::<Value>::new());
    assert!(finished_its_schedule(&outcome), "{:?}", outcome.messages);
    assert_eq!(outcome.temporary_entries_after_cycles, [Vec::<PathBuf>::new()]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_stop_between_two_cycles_reports_no_stopped_cycle_and_every_cycle_before_it_completed() {
    let mappings = Mappings::new();
    let data = mappings.write("stations.csv", STATIONS);
    let declared = format!(
        r#"{{ source: "{}", mapping: "{}" }}"#,
        data.display(),
        mappings.write("station.json5", STATION_MAPPING).display()
    );
    let schemas = RunSchemas::Permissive(&["Station"]);
    let setup = RunSetup {
        schedule: Some(r#"{ mode: "every", value: "1s" }"#),
        stop: RunStop::after_cycles(&STOPPED_AFTER_SECOND_CYCLE, 2),
        ..RunSetup::files(&schemas, "")
    };

    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &declared, &setup);

    result.unwrap();
    assert_eq!(cycle_lines(&outcome), ["#00001 completed", "#00002 completed"]);
    assert_eq!(outcome.errors, 0);
    assert!(finished_its_schedule(&outcome), "{:?}", outcome.messages);
}

#[test]
fn a_one_shot_run_stopped_while_collecting_says_it_was_stopped_and_counts_no_error() {
    let server = TwoPageServer::new(&ONE_SHOT_STOPPED_BETWEEN_PAGES);
    let mappings = Mappings::new();
    let declared = server.declared(&mappings.write("station.json5", THING_MAPPING));
    let schemas = RunSchemas::Permissive(&["Station"]);
    let setup = RunSetup {
        stop: RunStop::raised_by(&ONE_SHOT_STOPPED_BETWEEN_PAGES),
        ..RunSetup::files(&schemas, "")
    };

    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &declared, &setup);

    result.unwrap();
    server.second.assert();
    let stopped = outcome.messages.iter().filter(|message| message.starts_with("Run stopped (")).count();
    assert_eq!(stopped, 1, "{:?}", outcome.messages);
    assert_eq!(cycle_lines(&outcome), Vec::<String>::new());
    assert_eq!(outcome.errors, 0);
    assert_eq!(outcome.temporary_entries_after_cycles, [Vec::<PathBuf>::new()]);
    assert_eq!(outcome.temporary_entries_after_run, Vec::<PathBuf>::new());
}

#[test]
fn a_scheduled_cycle_that_fails_is_still_reported_failed_with_its_error() {
    let mappings = Mappings::new();
    let data = mappings.write("stations.csv", STATIONS);
    let refused = r#"{ version: "v4", dataModel: "Station", identity: { entityName: "{{ id }}" }, attributes: { id: { source: "{{ id }}" } } }"#;
    let declared = format!(
        r#"{{ source: "{}", mapping: "{}" }}"#,
        data.display(),
        mappings.write("station.json5", refused).display()
    );
    let schemas = RunSchemas::Permissive(&["Station"]);
    let setup = RunSetup {
        schedule: Some(r#"{ mode: "every", value: "1m", repeat: 1 }"#),
        on_failure: Some(r#""continue""#),
        ..RunSetup::files(&schemas, "")
    };

    let (result, outcome) = attempt_setup_end_to_end(StoreKind::DashMap, &declared, &setup);

    let error = result.unwrap_err();
    assert!(
        matches!(&error, PipelineError::Mapping(MappingError::ReservedAttributeName { attribute, .. }) if attribute.as_str() == "id"),
        "{error:?}"
    );
    let failed = outcome.coded(DiagnosticCode::Run(RunCode::CycleFailed));
    let [failed] = failed.as_slice() else {
        panic!("one cycle failed, so one failure is reported: {:?}", outcome.diagnostics);
    };
    assert!(failed.headline.ends_with("] #00001 failed"), "{}", failed.headline);
    assert!(failed.causes.iter().any(|cause| cause.contains("Attribute `id`")), "{:?}", failed.causes);
    assert_eq!(cycle_lines(&outcome), Vec::<String>::new());
    assert!(finished_its_schedule(&outcome), "{:?}", outcome.messages);
}

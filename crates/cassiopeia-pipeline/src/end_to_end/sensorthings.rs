use crate::{
    end_to_end::harness::{RunOutcome, RunSchemas, STORES, attempt_declared_end_to_end, attempt_file_end_to_end, attempt_scheduled_end_to_end, entity_ids},
    error::PipelineError,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};
use cassiopeia_sensorthings::read_kind::ReadKind;
use chrono::{DateTime, TimeDelta, Utc};
use mockito::{Matcher, Mock, Request, ServerGuard};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    iter,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
pub(crate) const SERIES_OUTPUT: &str = r#", temporal: { representation: "series" }"#;

/// The fixture `name` of the SensorThings snapshot tests.
pub(crate) fn fixture(name: &str) -> PathBuf {
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
pub(crate) fn entity<'a>(outcome: &'a RunOutcome, id: &str) -> &'a Value {
    outcome.entities.iter().find(|entity| entity["id"] == id).expect("the entity is written")
}

/// The `observedAt` of every instance of a series attribute, in the order written.
pub(crate) fn instant_times(attribute: &Value) -> Vec<&str> {
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
fn a_remote_latest_read_writes_one_entity_per_thing() {
    // The Datastream items of the RIVM latest fixture, as the server answered its discovery.
    let datastreams: Vec<Value> = fs::read_to_string(fixture("rivm-v1.0-latest.jsonl"))
        .unwrap()
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["datastream"].take())
        .collect();
    let page = json!({ "value": datastreams }).to_string();
    let mut server = mockito::Server::new();
    let declared = format!(
        r#"{{ source: "{}/v1.0", format: "sensorthings", mapping: "{}" }}"#,
        server.url(),
        fixture("air-quality-observed.json5").display()
    );
    let things = [
        "urn:ngsi-ld:AirQualityObserved:sta-127001-12511",
        "urn:ngsi-ld:AirQualityObserved:sta-127001-12512",
    ];

    for store in STORES {
        let discovery = server
            .mock(
                "GET",
                "/v1.0/Datastreams?$expand=Thing/Locations,ObservedProperty,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000",
            )
            .with_body(&page)
            .expect(1)
            .create();

        let (result, outcome) = attempt_declared_end_to_end(store, &declared, &RunSchemas::Folder(&fixture("schemas")), r#", validation: { mode: "fail" }"#);
        result.unwrap();

        discovery.assert();
        assert_eq!(entity_ids(&outcome), things, "{store:?}");
        for id in things {
            let station = entity(&outcome, id);
            for attribute in ["pm10", "pm25"] {
                assert_eq!(station[attribute]["observedAt"], RIVM_LATEST_TIME, "{store:?} {id} {attribute}");
                assert_eq!(station[attribute]["unitCode"], "GQ", "{store:?} {id} {attribute}");
            }
        }
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
        discovery.remove();
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

/// The path and query of the metadata discovery request of a range read of the mock server's
/// `/v1.0` root with the default settings.
const RANGE_METADATA: &str = "/v1.0/Datastreams?$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=1000";

/// The one Thing the RIVM range fixture holds, as its entity is named when read from the mock server.
pub(crate) const MOCK_RANGE_THING: &str = "urn:ngsi-ld:AirQualityObserved:sta-127001-12512";

/// The path and query of every observations request a mock server answered, in the order it
/// answered them.
#[derive(Clone, Default)]
struct AnsweredRequests(Arc<Mutex<Vec<String>>>);

impl AnsweredRequests {
    /// A body that answers a request with `body`, recording the request's path and query.
    fn answering(&self, body: String) -> impl Fn(&Request) -> Vec<u8> + Send + Sync + 'static {
        let answered = Arc::clone(&self.0);
        move |request| {
            answered.lock().unwrap().push(request.path_and_query().to_owned());
            body.clone().into_bytes()
        }
    }

    /// The `phenomenonTime` bounds of every answered request, in the order answered.
    fn bounds(&self) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        let instant = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().to_utc();
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|request| {
                let (_, filter) = request.split_once("phenomenonTime%20ge%20").unwrap();
                let (from, rest) = filter.split_once("%20and%20phenomenonTime%20lt%20").unwrap();
                let (to, _) = rest.split_once('&').unwrap();
                (instant(from), instant(to))
            })
            .collect()
    }
}

/// The input that reads the `last` span of readings of the mock server's `/v1.0` root through the
/// `AirQualityObserved` mapping.
fn remote_range(server: &ServerGuard, last: &str) -> String {
    format!(
        r#"{{ source: "{}/v1.0", format: "sensorthings", mapping: "{}", sensorthings: {{ read: {{ range: {{ last: "{last}" }} }} }} }}"#,
        server.url(),
        fixture("air-quality-observed.json5").display()
    )
}

/// Matches the observations requests of the data stream `id` of the mock server's `/v1.0` root.
fn observations_of(id: i64) -> Matcher {
    Matcher::Regex(format!(r"^/v1\.0/Datastreams\({id}\)/Observations\?"))
}

/// Serves the RIVM range fixture as the mock server's `/v1.0` root: its Datastreams to `discoveries`
/// metadata discoveries, and the Observations of each data stream `id` to `observation_reads(id)`
/// requests, recording each one in `answered`.
fn serve_rivm_range(server: &mut ServerGuard, discoveries: usize, observation_reads: impl Fn(i64) -> usize, answered: &AnsweredRequests) -> Vec<Mock> {
    let mut datastreams = Vec::new();
    let mut observations: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for line in fs::read_to_string(fixture("rivm-v1.0-range.jsonl")).unwrap().lines().skip(1) {
        let mut line: Value = serde_json::from_str(line).unwrap();
        match line.get_mut("observation").map(Value::take) {
            Some(observation) => observations.entry(line["datastream"].as_i64().unwrap()).or_default().push(observation),
            None => datastreams.push(line["datastream"].take()),
        }
    }
    let discovery = server
        .mock("GET", RANGE_METADATA)
        .with_body(json!({ "value": datastreams }).to_string())
        .expect(discoveries)
        .create();
    let reads = observations.into_iter().map(|(id, observations)| {
        server
            .mock("GET", observations_of(id))
            .with_body_from_request(answered.answering(json!({ "value": observations }).to_string()))
            .expect(observation_reads(id))
            .create()
    });
    iter::once(discovery).chain(reads.collect::<Vec<_>>()).collect()
}

#[test]
fn each_scheduled_run_reads_from_its_own_start() {
    let mut server = mockito::Server::new();
    let answered = AnsweredRequests::default();
    let mocks = serve_rivm_range(&mut server, 2, |_| 2, &answered);

    // The read under test is the same on either store, and the two runs are two seconds apart, as a
    // cycle start is held to the second.
    let (result, outcome) = attempt_scheduled_end_to_end(
        StoreKind::DashMap,
        &remote_range(&server, "1h"),
        &RunSchemas::Folder(&fixture("schemas")),
        SERIES_OUTPUT,
        r#"{ mode: "every", value: "2s", repeat: 2 }"#,
    );

    result.unwrap();
    mocks.iter().for_each(Mock::assert);
    let bounds = answered.bounds();
    assert_eq!(bounds.len(), 4, "{bounds:?}");
    assert_eq!((bounds[0], bounds[2]), (bounds[1], bounds[3]), "{bounds:?}");
    assert!(bounds[2].0 > bounds[0].0, "{bounds:?}");
    assert!(bounds.iter().all(|(from, to)| *to - *from == TimeDelta::hours(1)), "{bounds:?}");
    assert_eq!(entity_ids(&outcome).first().map(String::as_str), Some(MOCK_RANGE_THING));
}

#[test]
fn a_retried_cycle_reads_the_same_range() {
    let mut server = mockito::Server::new();
    let answered = AnsweredRequests::default();
    // Created first, so it answers the first three requests for the data stream's Observations: the
    // downloader's three attempts of the first cycle attempt.
    let failing = server
        .mock("GET", observations_of(56776))
        .with_status(500)
        .with_body_from_request(answered.answering(String::new()))
        .expect(3)
        .create();
    let mocks = serve_rivm_range(&mut server, 2, |id| if id == 56776 { 1 } else { 2 }, &answered);

    let (result, outcome) = attempt_scheduled_end_to_end(
        StoreKind::DashMap,
        &remote_range(&server, "24h"),
        &RunSchemas::Folder(&fixture("schemas")),
        SERIES_OUTPUT,
        r#"{ mode: "every", value: "1h", repeat: 1, retry: { maxAttempts: 2, backoff: "0s" } }"#,
    );

    result.unwrap();
    failing.assert();
    mocks.iter().for_each(Mock::assert);
    let bounds = answered.bounds();
    assert_eq!(bounds.len(), 6, "{bounds:?}");
    assert!(bounds.iter().all(|bound| *bound == bounds[0]), "{bounds:?}");
    assert_eq!(entity_ids(&outcome), [MOCK_RANGE_THING]);
    let station = entity(&outcome, MOCK_RANGE_THING);
    assert_eq!(
        instant_times(&station["pm10"]),
        ["2026-11-12T04:00:00Z", "2026-11-12T05:00:00Z", "2026-11-12T06:00:00Z"]
    );
}

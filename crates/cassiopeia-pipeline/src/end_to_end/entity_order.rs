use crate::end_to_end::harness::{
    RunBinding,
    RunInput,
    RunOutput,
    RunSchemas,
    RunSetup,
    STORES,
    attempt_setup_end_to_end,
    listed_ids,
    run_csv_end_to_end,
    run_end_to_end,
};
use mockito::{Matcher, Request};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    fs,
    iter,
    sync::{Arc, Mutex},
};
use temp_dir::TempDir;

/// The stations of the reproduction: five, whose records come in an order other than that of their
/// codes.
const STATIONS: &str = "code,name\nS3,Depot\nS1,Main\nS5,Yard\nS2,Branch\nS4,Annex\n";

/// The stations of [`STATIONS`], in the order their records come.
const STATIONS_IN_SOURCE_ORDER: [&str; 5] = [
    "urn:ngsi-ld:Station:S3",
    "urn:ngsi-ld:Station:S1",
    "urn:ngsi-ld:Station:S5",
    "urn:ngsi-ld:Station:S2",
    "urn:ngsi-ld:Station:S4",
];

const STATION_BY_CODE: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { name: { source: "{{ name }}" } },
}"#;

#[test]
fn a_current_state_file_lists_its_entities_in_the_order_of_their_first_records_on_every_run_and_store() {
    let mut runs = Vec::new();
    for store in STORES {
        for _ in 0..20 {
            let outcome = run_csv_end_to_end(store, STATIONS, &RunBinding::Mapping(STATION_BY_CODE), &["Station"], "");
            assert_eq!(listed_ids(&outcome.entities), STATIONS_IN_SOURCE_ORDER, "{store:?}");
            runs.push(outcome.files);
        }
    }
    assert!(runs.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn an_entity_whose_records_are_spread_through_the_source_is_written_at_the_place_of_its_first_record() {
    let stations = "code,name\nS2,Branch\nS1,Main\nS2,Branch\nS3,Depot\nS1,Main\nS4,Annex\nS3,Depot\n";
    for store in STORES {
        let outcome = run_csv_end_to_end(store, stations, &RunBinding::Mapping(STATION_BY_CODE), &["Station"], "");
        assert_eq!(
            listed_ids(&outcome.entities),
            [
                "urn:ngsi-ld:Station:S2",
                "urn:ngsi-ld:Station:S1",
                "urn:ngsi-ld:Station:S3",
                "urn:ngsi-ld:Station:S4"
            ],
            "{store:?}"
        );
    }
}

#[test]
fn a_run_of_two_inputs_writes_the_first_input_s_entities_before_the_second_s() {
    let inputs = [
        RunInput {
            binding: RunBinding::Mapping(STATION_BY_CODE),
            records: json!([{ "code": "S4", "name": "Annex" }, { "code": "S2", "name": "Branch" }]),
        },
        RunInput {
            binding: RunBinding::Mapping(STATION_BY_CODE),
            records: json!([
                { "code": "S3", "name": "Depot" },
                { "code": "S2", "name": "Branch" },
                { "code": "S1", "name": "Main" },
            ]),
        },
    ];
    for store in STORES {
        for _ in 0..10 {
            let outcome = run_end_to_end(store, &inputs, &["Station"], "");
            // `S2` has a record in both inputs, so it is written where its record in the first one
            // places it.
            assert_eq!(
                listed_ids(&outcome.entities),
                [
                    "urn:ngsi-ld:Station:S4",
                    "urn:ngsi-ld:Station:S2",
                    "urn:ngsi-ld:Station:S3",
                    "urn:ngsi-ld:Station:S1"
                ],
                "{store:?}"
            );
        }
    }
}

#[test]
fn a_broker_receives_the_entities_in_requests_that_each_hold_the_next_entities_of_the_file_once() {
    // Enough stations for several requests, their codes in an order other than that of their text.
    let codes: Vec<String> = (0..1000_usize).map(|record| format!("S{}", record * 389 % 1000)).collect();
    let stations = iter::once("code,name".to_owned())
        .chain(codes.iter().map(|code| format!("{code},Station {code}")))
        .collect::<Vec<_>>()
        .join("\n");
    let in_file_order: Vec<String> = codes.iter().map(|code| format!("urn:ngsi-ld:Station:{code}")).collect();
    let directory = TempDir::new().unwrap();
    let data = directory.path().join("stations.csv");
    let mapping = directory.path().join("station.json5");
    fs::write(&data, stations).unwrap();
    fs::write(&mapping, STATION_BY_CODE).unwrap();
    let declared = format!(r#"{{ source: "{}", mapping: "{}" }}"#, data.display(), mapping.display());

    for store in STORES {
        let mut broker = mockito::Server::new();
        let bodies: Arc<Mutex<Vec<Value>>> = Arc::default();
        let kept = Arc::clone(&bodies);
        let upsert = broker
            .mock("POST", Matcher::Regex(r"^/ngsi-ld/v1/entityOperations/upsert".to_owned()))
            .with_status(201)
            .with_body_from_request(move |request: &Request| {
                kept.lock().unwrap().push(serde_json::from_slice(request.body().unwrap()).unwrap());
                Vec::new()
            })
            .expect_at_least(2)
            .create();
        let output = format!(r#"{{ target: "context-broker", url: "{}/" }}"#, broker.url());
        let setup = RunSetup {
            output: RunOutput::Declared(&output),
            ..RunSetup::files(&RunSchemas::Permissive(&["Station"]), "")
        };

        let (result, _outcome) = attempt_setup_end_to_end(store, &declared, &setup);
        result.unwrap();

        upsert.assert();
        // Several requests can be in flight at once, so they are put back in the order of their first
        // entities in the file: each must then continue where the one before ended.
        let mut requests: Vec<Vec<String>> = bodies
            .lock()
            .unwrap()
            .iter()
            .map(|body| listed_ids(body.as_array().unwrap()).into_iter().map(str::to_owned).collect())
            .collect();
        requests.sort_by_key(|request| in_file_order.iter().position(|id| Some(id) == request.first()));
        assert_eq!(requests.concat(), in_file_order, "{store:?}");
    }
}

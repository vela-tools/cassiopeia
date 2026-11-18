//! The targets several records give one entity follow the order of the source on every run and
//! entity store: the order of the inputs in the manifest, then the order of the records within an
//! input, then the order in which a record's source lists them. A `ListRelationship` lists each
//! target once, at the place of the earliest record that names it, and a `Relationship` keeps the
//! target of the first record that names one.

use crate::end_to_end::harness::{RunBinding, RunInput, RunOutcome, STORES, run_csv_end_to_end, run_end_to_end};
use cassiopeia_common::store_kind::StoreKind;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

/// How many times each store runs the same input, so an order that changed between runs would show.
const RUNS: usize = 20;

/// Eight records of one stop, each naming one line.
const STOPS: &str = "code,lines\nS1,U1\nS1,U2\nS1,U3\nS1,U4\nS1,U5\nS1,U6\nS1,U7\nS1,U8\n";

/// A stop linked to every line its records name.
const STOP_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Stop",
    identity: { entityName: "{{ code }}" },
    attributes: {
        servedBy: { type: "ListRelationship", source: "{{ lines }}", target: { entity: "Line" } },
    },
}"#;

/// Four records of one sensor, each naming a different building.
const SENSORS: &str = "code,building\nS1,B1\nS1,B2\nS1,B3\nS1,B4\n";

/// A sensor linked to the building its records name.
const SENSOR_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: {
        building: { type: "Relationship", source: "{{ building }}", target: { entity: "Building" } },
    },
}"#;

/// The simplified representation, which writes a `ListRelationship` as the list of its targets.
const SIMPLIFIED: &str = r#", representation: "simplified""#;

/// The one entity `outcome` wrote.
fn only_entity(outcome: &RunOutcome) -> &Value {
    let [entity] = outcome.entities.as_slice() else {
        panic!("one entity must be written: {:?}", outcome.entities);
    };
    entity
}

/// Runs `run` [`RUNS`] times on each store, checks every entity it wrote with `check`, and asserts
/// that every run wrote the same files, byte for byte.
fn assert_every_run_and_store_writes_the_same(run: impl Fn(StoreKind) -> RunOutcome, check: impl Fn(&Value)) {
    let mut written: Vec<BTreeMap<PathBuf, String>> = Vec::new();
    for store in STORES {
        for _ in 0..RUNS {
            let outcome = run(store);
            check(only_entity(&outcome));
            written.push(outcome.files);
        }
    }
    assert_eq!(written.len(), 2 * RUNS);
    assert!(written.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn a_list_relationship_lists_the_targets_of_several_records_in_source_order_on_every_run_and_store() {
    assert_every_run_and_store_writes_the_same(
        |store| run_csv_end_to_end(store, STOPS, &RunBinding::Mapping(STOP_MAPPING), &["Stop"], SIMPLIFIED),
        |stop| {
            assert_eq!(
                stop["servedBy"],
                json!([
                    "urn:ngsi-ld:Line:U1",
                    "urn:ngsi-ld:Line:U2",
                    "urn:ngsi-ld:Line:U3",
                    "urn:ngsi-ld:Line:U4",
                    "urn:ngsi-ld:Line:U5",
                    "urn:ngsi-ld:Line:U6",
                    "urn:ngsi-ld:Line:U7",
                    "urn:ngsi-ld:Line:U8",
                ])
            );
        },
    );
}

#[test]
fn a_relationship_keeps_the_target_of_the_first_record_on_every_run_and_store() {
    assert_every_run_and_store_writes_the_same(
        |store| run_csv_end_to_end(store, SENSORS, &RunBinding::Mapping(SENSOR_MAPPING), &["Sensor"], ""),
        |sensor| assert_eq!(sensor["building"]["object"], "urn:ngsi-ld:Building:B1"),
    );
}

#[test]
fn a_list_relationship_over_two_inputs_lists_the_first_input_s_targets_first_on_every_run_and_store() {
    let inputs = [
        RunInput {
            binding: RunBinding::Mapping(STOP_MAPPING),
            records: json!([{ "code": "S1", "lines": "U1" }, { "code": "S1", "lines": "U2" }]),
        },
        RunInput {
            binding: RunBinding::Mapping(STOP_MAPPING),
            records: json!([{ "code": "S1", "lines": "U3" }, { "code": "S1", "lines": "U4" }]),
        },
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run_end_to_end(store, &inputs, &["Stop"], SIMPLIFIED),
        |stop| {
            assert_eq!(
                stop["servedBy"],
                json!(["urn:ngsi-ld:Line:U1", "urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U3", "urn:ngsi-ld:Line:U4"])
            );
        },
    );
}

#[test]
fn a_target_several_records_name_is_listed_once_at_the_place_of_the_earliest_of_them() {
    assert_every_run_and_store_writes_the_same(
        |store| {
            run_csv_end_to_end(
                store,
                "code,lines\nS1,U2\nS1,U1 U2\n",
                &RunBinding::Mapping(STOP_MAPPING),
                &["Stop"],
                SIMPLIFIED,
            )
        },
        |stop| assert_eq!(stop["servedBy"], json!(["urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U1"])),
    );
}

#[test]
fn a_temporal_mapping_whose_records_repeat_a_target_lists_it_at_the_place_of_the_earliest_of_them() {
    let mapping = r#"{
        version: "v4",
        dataModel: "Stop",
        identity: { entityName: "{{ code }}" },
        attributes: {
            occupancy: { source: "{{ occupancy }}", transformation: "integer", properties: { observedAt: { source: "{{ time }}" } } },
            servedBy: { type: "ListRelationship", source: "{{ lines }}", target: { entity: "Line" } },
        },
    }"#;
    // The records repeat `U2` before and after naming `U1`, and the newest record names neither
    // first.
    let records = "code,occupancy,time,lines\n\
                   S1,4,2026-03-01T10:00:00Z,U2\n\
                   S1,5,2026-03-01T11:00:00Z,U1 U2\n\
                   S1,6,2026-03-01T12:00:00Z,U3 U2\n\
                   S1,7,2026-03-01T13:00:00Z,U1\n";
    assert_every_run_and_store_writes_the_same(
        |store| run_csv_end_to_end(store, records, &RunBinding::Mapping(mapping), &["Stop"], SIMPLIFIED),
        |stop| {
            assert_eq!(stop["occupancy"], 7);
            assert_eq!(stop["servedBy"], json!(["urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U1", "urn:ngsi-ld:Line:U3"]));
        },
    );
}

#[test]
fn the_instances_and_the_nested_targets_of_several_records_follow_the_source_on_every_run_and_store() {
    let mapping = r#"{
        version: "v4",
        dataModel: "Route",
        identity: { entityName: "{{ code }}" },
        attributes: {
            status: {
                source: "{{ status }}",
                properties: {
                    operators: { type: "ListRelationship", source: "{{ operators }}", target: { entity: "Operator" } },
                },
            },
            servesAirports: {
                type: "ListRelationship",
                target: { entity: "Airport" },
                instances: [
                    { source: "{{ primary }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:primary" } } },
                    { source: "{{ backup }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:backup" } } },
                ],
            },
        },
    }"#;
    let records = "code,status,operators,primary,backup\n\
                   R1,active,O2,A3 A1,A9\n\
                   R1,active,O1 O2,A2 A3,A8 A9\n\
                   R1,active,O3,A1,A7\n";
    assert_every_run_and_store_writes_the_same(
        |store| run_csv_end_to_end(store, records, &RunBinding::Mapping(mapping), &["Route"], ""),
        |route| {
            assert_eq!(
                route["status"]["operators"]["objectList"],
                json!([
                    { "object": "urn:ngsi-ld:Operator:O2" },
                    { "object": "urn:ngsi-ld:Operator:O1" },
                    { "object": "urn:ngsi-ld:Operator:O3" },
                ])
            );
            let lists: Vec<&Value> = route["servesAirports"]
                .as_array()
                .unwrap()
                .iter()
                .map(|instance| &instance["objectList"])
                .collect();
            assert_eq!(
                lists,
                [
                    &json!([
                        { "object": "urn:ngsi-ld:Airport:A3" },
                        { "object": "urn:ngsi-ld:Airport:A1" },
                        { "object": "urn:ngsi-ld:Airport:A2" },
                    ]),
                    &json!([
                        { "object": "urn:ngsi-ld:Airport:A9" },
                        { "object": "urn:ngsi-ld:Airport:A8" },
                        { "object": "urn:ngsi-ld:Airport:A7" },
                    ]),
                ]
            );
        },
    );
}

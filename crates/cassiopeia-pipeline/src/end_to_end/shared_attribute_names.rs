//! Mappings of one manifest that contribute to one entity and declare an attribute of the same name
//! write it whole from the first mapping in the manifest whose record gives it a value, on every run
//! and entity store: the value, the declaration it is built with, and its sub-attributes all come
//! from that mapping. A relationship keeps the targets every mapping gives it, with the declaration
//! and sub-attributes of the first mapping that declares it.

use crate::end_to_end::{
    harness::{RunBinding, RunInput, RunOutcome, run_end_to_end},
    repeated_runs::{assert_every_run_and_store_writes_the_same, only_entity},
};
use cassiopeia_common::store_kind::StoreKind;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A station with the label its record gives it.
const LABELLED: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { label: { source: "{{ label }}" } },
}"#;

/// A station with the label and the zone its record gives it.
const LABELLED_AND_ZONED: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { label: { source: "{{ label }}" }, zone: { source: "{{ zone }}" } },
}"#;

/// A station counting as an integer.
const COUNTED: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { count: { source: "{{ count }}", transformation: "integer" } },
}"#;

/// A station counting as text.
const COUNTED_AS_TEXT: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { count: { source: "{{ count }}" } },
}"#;

/// A station reading its temperature in degrees Celsius.
const CELSIUS: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { temperature: { source: "{{ t }}", transformation: "float", properties: { unitCode: { source: "CEL" } } } },
}"#;

/// A station reading its temperature in degrees Fahrenheit at the time its record names.
const FAHRENHEIT: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: {
        temperature: {
            source: "{{ t }}",
            transformation: "float",
            properties: { unitCode: { source: "FAH" }, observedAt: { source: "{{ time }}" } },
        },
    },
}"#;

/// A station reading its temperature with no qualifier.
const UNQUALIFIED: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { temperature: { source: "{{ t }}", transformation: "float" } },
}"#;

/// A sensor whose reading and status are each observed at a time of their own.
const OBSERVED_SENSOR: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: {
        reading: { source: "{{ reading }}", transformation: "float", properties: { observedAt: { source: "{{ read_time }}" } } },
        status: { source: "{{ status }}", properties: { observedAt: { source: "{{ status_time }}" } } },
    },
}"#;

/// A sensor with a status and a label no time qualifies.
const STATIC_SENSOR: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: { status: { source: "{{ status }}" }, label: { source: "{{ label }}" } },
}"#;

/// A stop served by the lines its record names, in the role its record names.
const SERVED_IN_A_ROLE: &str = r#"{
    version: "v4",
    dataModel: "Stop",
    identity: { entityName: "{{ code }}" },
    attributes: {
        servedBy: { type: "ListRelationship", source: "{{ lines }}", target: { entity: "Line" }, properties: { role: { source: "{{ role }}" } } },
    },
}"#;

/// A stop served by the lines its record names, with the note its record gives.
const SERVED_WITH_A_NOTE: &str = r#"{
    version: "v4",
    dataModel: "Stop",
    identity: { entityName: "{{ code }}" },
    attributes: {
        servedBy: { type: "ListRelationship", source: "{{ lines }}", target: { entity: "Line" }, properties: { note: { source: "{{ note }}" } } },
    },
}"#;

/// One input mapped by `mapping`, reading `records`.
const fn input(mapping: &'static str, records: Value) -> RunInput {
    RunInput {
        binding: RunBinding::Mapping(mapping),
        records,
    }
}

/// Runs `inputs` on `store`, validating against an accept-anything schema for `entity_type`.
fn run(store: StoreKind, inputs: &[RunInput], entity_type: &str) -> RunOutcome {
    run_end_to_end(store, inputs, &[entity_type], "")
}

/// The normalized targets of a `ListRelationship`, one object member per target.
fn object_list(lines: &[&str]) -> Value {
    json!(
        lines
            .iter()
            .map(|line| json!({ "object": format!("urn:ngsi-ld:Line:{line}") }))
            .collect::<Vec<_>>()
    )
}

#[test]
fn a_name_two_mappings_both_give_a_value_keeps_the_first_mapping_s_value_on_every_run_and_store() {
    let inputs = [
        input(LABELLED, json!([{ "code": "S1", "label": "North" }])),
        input(LABELLED_AND_ZONED, json!([{ "code": "S1", "label": "South", "zone": "B" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| {
            let station = only_entity(outcome);
            assert_eq!(station["label"]["value"], "North");
            assert_eq!(station["zone"]["value"], "B");
        },
    );
}

#[test]
fn a_name_the_first_mapping_s_record_lacks_is_taken_from_the_second_mapping_on_every_run_and_store() {
    let inputs = [
        input(LABELLED, json!([{ "code": "S1" }])),
        input(LABELLED_AND_ZONED, json!([{ "code": "S1", "label": "South", "zone": "B" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| assert_eq!(only_entity(outcome)["label"]["value"], "South"),
    );
}

#[test]
fn a_name_declared_differently_keeps_the_first_mapping_s_integer_when_both_give_a_value_on_every_run_and_store() {
    let inputs = [
        input(COUNTED, json!([{ "code": "S1", "count": "7" }])),
        input(COUNTED_AS_TEXT, json!([{ "code": "S1", "count": "seven" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| assert_eq!(only_entity(outcome)["count"], json!({ "type": "Property", "value": 7 })),
    );
}

#[test]
fn a_name_declared_differently_is_built_as_the_second_mapping_declares_it_when_only_the_second_gives_a_value_on_every_run_and_store() {
    let inputs = [
        input(COUNTED, json!([{ "code": "S1" }])),
        input(COUNTED_AS_TEXT, json!([{ "code": "S1", "count": "seven" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| assert_eq!(only_entity(outcome)["count"], json!({ "type": "Property", "value": "seven" })),
    );
}

#[test]
fn the_first_mapping_s_value_carries_only_its_own_qualifiers_on_every_run_and_store() {
    let inputs = [
        input(CELSIUS, json!([{ "code": "S1", "t": 21.5 }])),
        input(FAHRENHEIT, json!([{ "code": "S1", "t": 70.7, "time": "2026-03-01T12:00:00Z" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| {
            assert_eq!(
                only_entity(outcome)["temperature"],
                json!({ "type": "Property", "value": 21.5, "unitCode": "CEL" })
            );
        },
    );
}

#[test]
fn the_first_mapping_s_value_carries_only_its_own_qualifiers_when_the_manifest_lists_the_other_mapping_first_on_every_run_and_store() {
    let inputs = [
        input(FAHRENHEIT, json!([{ "code": "S1", "t": 70.7, "time": "2026-03-01T12:00:00Z" }])),
        input(CELSIUS, json!([{ "code": "S1", "t": 21.5 }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| {
            assert_eq!(
                only_entity(outcome)["temperature"],
                json!({ "type": "Property", "value": 70.7, "unitCode": "FAH", "observedAt": "2026-03-01T12:00:00Z" })
            );
        },
    );
}

#[test]
fn a_value_the_second_mapping_gives_carries_only_the_second_mapping_s_qualifiers_on_every_run_and_store() {
    let inputs = [
        input(CELSIUS, json!([{ "code": "S1" }])),
        input(FAHRENHEIT, json!([{ "code": "S1", "t": 70.7, "time": "2026-03-01T12:00:00Z" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| {
            assert_eq!(
                only_entity(outcome)["temperature"],
                json!({ "type": "Property", "value": 70.7, "unitCode": "FAH", "observedAt": "2026-03-01T12:00:00Z" })
            );
        },
    );
}

#[test]
fn a_qualifier_of_a_mapping_that_gives_no_value_is_not_carried_onto_another_mapping_s_value_on_every_run_and_store() {
    let inputs = [
        input(CELSIUS, json!([{ "code": "S1" }])),
        input(UNQUALIFIED, json!([{ "code": "S1", "t": 70.7 }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Station"),
        |outcome| assert_eq!(only_entity(outcome)["temperature"], json!({ "type": "Property", "value": 70.7 })),
    );
}

/// Two records of the sensor `S1` whose reading and status were observed last in different records,
/// so its mapping keeps each attribute from a record of its own: the reading at 11:00 and the status
/// at 12:00. `status` is `None` to leave the status out of the record that observed it last.
fn observed_sensor_records(status: Option<&str>) -> Value {
    let mut latest_status = json!({ "code": "S1", "reading": 21.5, "read_time": "2026-03-01T10:00:00Z", "status_time": "2026-03-01T12:00:00Z" });
    if let Some(status) = status {
        latest_status["status"] = json!(status);
    }
    json!([
        latest_status,
        { "code": "S1", "reading": 22.5, "read_time": "2026-03-01T11:00:00Z", "status": "warm", "status_time": "2026-03-01T09:00:00Z" },
    ])
}

/// The one record of the static sensor `S1`.
fn static_sensor_records() -> Value {
    json!([{ "code": "S1", "status": "maintenance", "label": "Main" }])
}

/// The reading the observed sensor keeps from its record observed last, unchanged by any other
/// mapping.
fn kept_reading() -> Value {
    json!({ "type": "Property", "value": 22.5, "observedAt": "2026-03-01T11:00:00Z" })
}

#[test]
fn a_name_a_temporal_mapping_listed_first_gives_a_value_keeps_its_ranked_value_over_a_static_mapping_s_on_every_run_and_store() {
    let inputs = [
        input(OBSERVED_SENSOR, observed_sensor_records(Some("ok"))),
        input(STATIC_SENSOR, static_sensor_records()),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Sensor"),
        |outcome| {
            let sensor = only_entity(outcome);
            assert_eq!(
                sensor["status"],
                json!({ "type": "Property", "value": "ok", "observedAt": "2026-03-01T12:00:00Z" })
            );
            assert_eq!(sensor["reading"], kept_reading());
            assert_eq!(sensor["label"]["value"], "Main");
        },
    );
}

#[test]
fn a_name_a_static_mapping_listed_first_gives_a_value_keeps_its_value_over_a_temporal_mapping_s_on_every_run_and_store() {
    let inputs = [
        input(STATIC_SENSOR, static_sensor_records()),
        input(OBSERVED_SENSOR, observed_sensor_records(Some("ok"))),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Sensor"),
        |outcome| {
            let sensor = only_entity(outcome);
            assert_eq!(sensor["status"], json!({ "type": "Property", "value": "maintenance" }));
            assert_eq!(sensor["reading"], kept_reading());
        },
    );
}

#[test]
fn a_name_the_record_a_temporal_mapping_keeps_leaves_out_is_taken_from_a_static_mapping_listed_later_on_every_run_and_store() {
    let inputs = [
        input(OBSERVED_SENSOR, observed_sensor_records(None)),
        input(STATIC_SENSOR, static_sensor_records()),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Sensor"),
        |outcome| {
            let sensor = only_entity(outcome);
            assert_eq!(sensor["status"], json!({ "type": "Property", "value": "maintenance" }));
            assert_eq!(sensor["reading"], kept_reading());
        },
    );
}

#[test]
fn a_relationship_two_mappings_declare_joins_their_targets_with_the_first_mapping_s_properties_on_every_run_and_store() {
    let inputs = [
        input(SERVED_IN_A_ROLE, json!([{ "code": "S1", "lines": "U1 U2", "role": "primary" }])),
        input(SERVED_WITH_A_NOTE, json!([{ "code": "S1", "lines": "U2 U3", "note": "night" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Stop"),
        |outcome| {
            assert_eq!(
                only_entity(outcome)["servedBy"],
                json!({ "type": "ListRelationship", "objectList": object_list(&["U1", "U2", "U3"]), "objectType": "Line", "role": { "type": "Property", "value": "primary" } })
            );
        },
    );
}

#[test]
fn a_relationship_two_mappings_declare_takes_the_properties_of_the_mapping_the_manifest_lists_first_on_every_run_and_store() {
    let inputs = [
        input(SERVED_WITH_A_NOTE, json!([{ "code": "S1", "lines": "U2 U3", "note": "night" }])),
        input(SERVED_IN_A_ROLE, json!([{ "code": "S1", "lines": "U1 U2", "role": "primary" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Stop"),
        |outcome| {
            assert_eq!(
                only_entity(outcome)["servedBy"],
                json!({ "type": "ListRelationship", "objectList": object_list(&["U2", "U3", "U1"]), "objectType": "Line", "note": { "type": "Property", "value": "night" } })
            );
        },
    );
}

#[test]
fn a_relationship_keeps_the_first_declaring_mapping_s_properties_when_only_a_later_mapping_names_targets_on_every_run_and_store() {
    let inputs = [
        input(SERVED_IN_A_ROLE, json!([{ "code": "S1", "role": "primary" }])),
        input(SERVED_WITH_A_NOTE, json!([{ "code": "S1", "lines": "U2 U3", "note": "night" }])),
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run(store, &inputs, "Stop"),
        |outcome| {
            assert_eq!(
                only_entity(outcome)["servedBy"],
                json!({ "type": "ListRelationship", "objectList": object_list(&["U2", "U3"]), "objectType": "Line", "role": { "type": "Property", "value": "primary" } })
            );
        },
    );
}

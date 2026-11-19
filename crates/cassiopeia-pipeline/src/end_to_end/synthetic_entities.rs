//! A synthetic entity that the mappings of several inputs name, each with a value of its own, takes
//! the value of the mapping the manifest lists first, on every run and entity store, whichever
//! input's records reach the resolver first.

use crate::end_to_end::{
    harness::{RunBinding, RunInput, run_end_to_end},
    repeated_runs::assert_every_run_and_store_writes_the_same,
};
use pretty_assertions::assert_eq;
use serde_json::json;

/// A station operated by the operator its record names, with the label its record gives it.
const STATION: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: {
        operatedBy: {
            type: "Relationship",
            source: "{{ operator }}",
            target: { entity: "Operator" },
            syntheticEntity: { dataModel: "Operator", identity: { entityName: "{{ operator }}" }, attributes: { label: { source: "{{ label }}" } } },
        },
    },
}"#;

/// A depot operated by the operator its record names, with the label its record gives it.
const DEPOT: &str = r#"{
    version: "v4",
    dataModel: "Depot",
    identity: { entityName: "{{ code }}" },
    attributes: {
        operatedBy: {
            type: "Relationship",
            source: "{{ operator }}",
            target: { entity: "Operator" },
            syntheticEntity: { dataModel: "Operator", identity: { entityName: "{{ operator }}" }, attributes: { label: { source: "{{ label }}" } } },
        },
    },
}"#;

#[test]
fn a_synthetic_entity_two_inputs_name_with_different_values_takes_the_first_input_s_value_on_every_run_and_store() {
    let records = |prefix: &str, label: &str| {
        json!(
            (0..200)
                .map(|record| json!({ "code": format!("{prefix}{record}"), "operator": "O1", "label": label }))
                .collect::<Vec<_>>()
        )
    };
    let inputs = [
        RunInput {
            binding: RunBinding::Mapping(STATION),
            records: records("S", "City"),
        },
        RunInput {
            binding: RunBinding::Mapping(DEPOT),
            records: records("D", "Region"),
        },
    ];
    assert_every_run_and_store_writes_the_same(
        |store| run_end_to_end(store, &inputs, &["Station", "Depot", "Operator"], ""),
        |outcome| {
            let operator = outcome
                .entities
                .iter()
                .find(|entity| entity["id"] == "urn:ngsi-ld:Operator:O1")
                .unwrap_or_else(|| panic!("no operator was written: {:?}", outcome.entities));
            assert_eq!(operator["label"]["value"], "City");
        },
    );
}

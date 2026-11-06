use crate::end_to_end::harness::{RunBinding, RunInput, STORES, run_end_to_end};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn a_declared_value_type_reaches_the_written_entity_as_given_beside_the_one_a_time_implies() {
    let mapping = r#"{
        version: "v4",
        dataModel: "Place",
        identity: { entityName: "{{ id }}" },
        attributes: {
            address: {
                source: "{{ address }}",
                valueType: "PostalAddress",
                properties: { formatted: { source: "{{ address }}", valueType: "xsd:string" } },
            },
            openedAt: { source: "{{ opened }}", transformation: "datetime", valueType: "ngsi-ld:DateTime" },
            closedAt: { source: "{{ closed }}", transformation: "datetime" },
        },
    }"#;
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(mapping),
                records: json!([{ "id": "P1", "address": "Pariser Platz", "opened": "2026-10-24T08:00:00Z", "closed": "2026-10-24T20:00:00Z" }]),
            }],
            &["Place"],
            "",
        );

        let place = &outcome.entities[0];
        assert_eq!(place["address"]["valueType"], "PostalAddress", "{store:?}");
        assert_eq!(place["address"]["formatted"]["valueType"], "xsd:string", "{store:?}");
        assert_eq!(place["openedAt"]["valueType"], "ngsi-ld:DateTime", "{store:?}");
        assert_eq!(place["closedAt"]["valueType"], "DateTime", "{store:?}");
    }
}

use crate::end_to_end::harness::{RunBinding, RunInput, STORES, entity_ids, run_end_to_end};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// One departure per record, named after the stop and the line that enclose it.
const DEPARTURE: &str = r#"{
    version: "v4",
    dataModel: "Departure",
    identity: { entityName: "{{ monitors.locationStop.properties.attributes.rbl }}-{{ lines.name }}-{{ departureTime.countdown }}" },
    attributes: {
        line: { source: "{{ lines.name }}" },
        countdown: { source: "{{ departureTime.countdown }}" },
    },
}"#;

/// One line per record, named after the line itself.
const LINE: &str = r#"{
    version: "v4",
    dataModel: "Line",
    identity: { entityName: "{{ name }}" },
    attributes: { towards: { source: "{{ towards }}" } },
}"#;

/// A departure monitor shaped like the Wiener Linien real-time response: departures three arrays
/// below the root, which has no array member of its own.
fn monitor() -> Value {
    json!({
        "data": {
            "monitors": [{
                "locationStop": { "properties": { "title": "Schwedenplatz", "attributes": { "rbl": 4116 } } },
                "lines": [
                    {
                        "name": "U1",
                        "towards": "Oberlaa",
                        "departures": { "departure": [{ "departureTime": { "countdown": 5 } }, { "departureTime": { "countdown": 9 } }] }
                    },
                    {
                        "name": "U4",
                        "towards": "Hütteldorf",
                        "departures": { "departure": [{ "departureTime": { "countdown": 2 } }] }
                    }
                ]
            }]
        },
        "message": { "value": "OK" }
    })
}

#[test]
fn a_nested_json_collection_maps_one_entity_per_item_reading_its_enclosing_items() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Collections(&[("data.monitors.lines.departures.departure", DEPARTURE)]),
                records: monitor(),
            }],
            &["Departure"],
            "",
        );

        assert_eq!(
            entity_ids(&outcome),
            [
                "urn:ngsi-ld:Departure:4116-U1-5",
                "urn:ngsi-ld:Departure:4116-U1-9",
                "urn:ngsi-ld:Departure:4116-U4-2"
            ],
            "{store:?}"
        );
        let u4 = outcome
            .entities
            .iter()
            .find(|entity| entity["id"] == "urn:ngsi-ld:Departure:4116-U4-2")
            .unwrap();
        assert_eq!(u4["line"]["value"], "U4", "{store:?}: {u4:?}");
        assert_eq!(u4["countdown"]["value"], "2", "{store:?}: {u4:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn two_nested_json_collections_route_to_their_own_mappings_without_warning_for_the_unlisted_ones() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Collections(&[("data.monitors.lines", LINE), ("data.monitors.lines.departures.departure", DEPARTURE)]),
                records: monitor(),
            }],
            &["Departure", "Line"],
            "",
        );

        assert_eq!(
            entity_ids(&outcome),
            [
                "urn:ngsi-ld:Departure:4116-U1-5",
                "urn:ngsi-ld:Departure:4116-U1-9",
                "urn:ngsi-ld:Departure:4116-U4-2",
                "urn:ngsi-ld:Line:U1",
                "urn:ngsi-ld:Line:U4"
            ],
            "{store:?}"
        );
        let u1 = outcome.entities.iter().find(|entity| entity["id"] == "urn:ngsi-ld:Line:U1").unwrap();
        assert_eq!(u1["towards"]["value"], "Oberlaa", "{store:?}: {u1:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_single_json_mapping_keeps_reading_an_envelope_under_the_lone_array_convention() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(LINE),
                records: json!({ "generated": "2026-10-25", "items": [{ "name": "U1", "towards": "Oberlaa" }, { "name": "U4", "towards": "Hütteldorf" }] }),
            }],
            &["Line"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Line:U1", "urn:ngsi-ld:Line:U4"], "{store:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_single_json_mapping_keeps_a_nested_document_as_one_record() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(LINE),
                records: json!({ "name": "U1", "towards": "Oberlaa", "inbound": [{ "stop": 1 }], "outbound": [{ "stop": 2 }] }),
            }],
            &["Line"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Line:U1"], "{store:?}");
    }
}

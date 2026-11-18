use crate::end_to_end::harness::{RunBinding, RunInput, STORES, entity_ids, run_csv_end_to_end, run_end_to_end};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn a_relationship_whose_source_template_fails_is_left_out_with_a_warning_and_the_entity_still_published() {
    let mapping = r#"{
        version: "v4",
        dataModel: "Thing",
        identity: { entityName: "{{ id }}" },
        attributes: {
            name: { source: "{{ name }}" },
            refRoad: { type: "Relationship", source: "{{ road | lower }}", target: { entity: "Road" } },
        },
    }"#;
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(mapping),
                records: json!([{ "id": "A", "name": "Alpha", "road": "R1" }, { "id": "B", "name": "Beta", "road": null }]),
            }],
            &["Thing"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Thing:A", "urn:ngsi-ld:Thing:B"], "{store:?}");
        let beta = outcome.entities.iter().find(|entity| entity["id"] == "urn:ngsi-ld:Thing:B").unwrap();
        assert_eq!(beta["name"]["value"], "Beta", "{store:?}: {beta:?}");
        assert!(beta.get("refRoad").is_none(), "{store:?}: {beta:?}");
        let alpha = outcome.entities.iter().find(|entity| entity["id"] == "urn:ngsi-ld:Thing:A").unwrap();
        assert_eq!(alpha["refRoad"]["object"], "urn:ngsi-ld:Road:r1", "{store:?}: {alpha:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn a_list_relationship_is_written_with_each_target_as_an_object_member_in_the_normalized_representation() {
    let mapping = r#"{
        version: "v4",
        dataModel: "Stop",
        identity: { entityName: "{{ code }}" },
        attributes: {
            servedBy: { type: "ListRelationship", source: "{{ lines }}", target: { entity: "Line" } },
        },
    }"#;
    for store in STORES {
        let outcome = run_csv_end_to_end(store, "code,lines\nS1,U1 U4\n", &RunBinding::Mapping(mapping), &["Stop"], "");

        // ETSI GS CIM 009 v1.9.1 clause 4.5.22.2: the normalized `objectList` holds one
        // `{ "object": URI }` object per target, not the concise bare strings.
        assert_eq!(
            outcome.entities,
            [json!({
                "id": "urn:ngsi-ld:Stop:S1",
                "type": "Stop",
                "servedBy": {
                    "type": "ListRelationship",
                    "objectList": [{ "object": "urn:ngsi-ld:Line:U1" }, { "object": "urn:ngsi-ld:Line:U4" }],
                    "objectType": "Line",
                },
            })],
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
    }
}

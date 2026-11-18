use crate::end_to_end::harness::{RunBinding, RunInput, STORES, entity_ids, run_csv_end_to_end, run_end_to_end};
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode};
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

/// Three stations with their owners: `S1` fully described, a second station with no `station` to
/// name it, and `S3` whose owner's zone, `Graz Mitte`, is no legal scope level.
const OWNED_STATIONS: &str = "station,name,owner_id,owner_name,owner_zone
S1,Prater,O1,Wiener Linien,Wien
,Unnamed,O2,ÖBB,Wien
S3,Hbf,O3,Graz Holding,Graz Mitte
";

/// A station linked to the organization that owns it, which the same record describes and scopes by
/// its zone.
const OWNED_STATION_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ station }}" },
    attributes: {
        name: { source: "{{ name }}" },
        owner: {
            type: "Relationship",
            syntheticEntity: {
                dataModel: "Organization",
                identity: { entityName: "{{ owner_id }}" },
                scope: "/{{ owner_zone }}",
                attributes: { name: { source: "{{ owner_name }}" } },
            },
        },
    },
}"#;

#[test]
fn each_entity_of_a_record_is_written_on_its_own_when_another_entity_of_the_record_fails() {
    for store in STORES {
        let outcome = run_csv_end_to_end(
            store,
            OWNED_STATIONS,
            &RunBinding::Mapping(OWNED_STATION_MAPPING),
            &["Station", "Organization"],
            "",
        );

        assert_eq!(
            entity_ids(&outcome),
            [
                "urn:ngsi-ld:Organization:O1",
                "urn:ngsi-ld:Organization:O2",
                "urn:ngsi-ld:Station:S1",
                "urn:ngsi-ld:Station:S3",
            ],
            "{store:?}"
        );
        let entity = |id: &str| outcome.entities.iter().find(|entity| entity["id"] == id).unwrap();
        assert_eq!(entity("urn:ngsi-ld:Station:S1")["owner"]["object"], "urn:ngsi-ld:Organization:O1", "{store:?}");
        let orphaned_owner = entity("urn:ngsi-ld:Organization:O2");
        assert_eq!(orphaned_owner["scope"], "/Wien", "{store:?}: {orphaned_owner}");
        assert_eq!(orphaned_owner["name"]["value"], "ÖBB", "{store:?}: {orphaned_owner}");
        let unowned_station = entity("urn:ngsi-ld:Station:S3");
        assert_eq!(unowned_station["name"]["value"], "Hbf", "{store:?}: {unowned_station}");
        assert!(unowned_station.get("owner").is_none(), "{store:?}: {unowned_station}");

        let unnamed = outcome.coded(DiagnosticCode::Expander(ExpanderCode::IdAbsent));
        assert_eq!(unnamed.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(unnamed[0].occurrences, 1, "{store:?}");
        let illegal_scope = outcome.coded(DiagnosticCode::Expander(ExpanderCode::ScopeUnreadable));
        assert_eq!(illegal_scope.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert!(illegal_scope[0].headline.contains("/Graz Mitte"), "{store:?}: {}", illegal_scope[0].headline);
        assert_eq!(illegal_scope[0].occurrences, 1, "{store:?}");
        assert_eq!(outcome.warnings, 2, "{store:?}: {:?}", outcome.diagnostics);
    }
}

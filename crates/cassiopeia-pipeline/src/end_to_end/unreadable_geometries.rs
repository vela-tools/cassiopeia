use crate::end_to_end::harness::{RunBinding, RunInput, RunOutcome, STORES, entity_ids, run_end_to_end};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Three top-level `GeoProperty` attributes and one `GeoProperty` sub-attribute, each reading a
/// field of its own, so every shape of value can be put under every position in one record.
const THING_LOCATIONS: &str = r#"{
    version: "v4",
    dataModel: "Thing",
    identity: { entityName: "{{ id }}" },
    attributes: {
        locText: { type: "GeoProperty", source: "{{ gt }}" },
        locObj: { type: "GeoProperty", source: "{{ go }}" },
        locNum: { type: "GeoProperty", source: "{{ gn }}" },
        temperature: {
            source: "{{ t }}",
            properties: { where: { type: "GeoProperty", source: "{{ go }}" } },
        },
    },
}"#;

/// Runs one `Thing` record through [`THING_LOCATIONS`].
fn run_thing(store: StoreKind, record: &Value) -> RunOutcome {
    run_end_to_end(
        store,
        &[RunInput {
            binding: RunBinding::Mapping(THING_LOCATIONS),
            records: json!([record]),
        }],
        &["Thing"],
        "",
    )
}

#[test]
fn a_geo_property_over_a_value_that_is_no_geometry_is_dropped_with_a_warning_naming_the_attribute_and_the_value() {
    for store in STORES {
        let outcome = run_thing(store, &json!({ "id": "A", "t": 20, "gt": "not geojson", "go": { "type": "Point" }, "gn": 42 }));

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Thing:A"], "{store:?}");
        let entity = &outcome.entities[0];
        for dropped in ["locText", "locObj", "locNum"] {
            assert!(entity.get(dropped).is_none(), "{store:?} {dropped}: {entity:?}");
        }
        assert_eq!(entity["temperature"]["value"], "20", "{store:?}: {entity:?}");
        assert!(entity["temperature"].get("where").is_none(), "{store:?}: {entity:?}");

        let mut headlines = outcome
            .coded(DiagnosticCode::Extractor(ExtractorCode::GeometryUnreadable))
            .into_iter()
            .map(|diagnostic| diagnostic.headline.as_str())
            .collect::<Vec<&str>>();
        headlines.sort_unstable();
        assert_eq!(
            headlines,
            [
                "Attribute `locNum` dropped: `42` is not a GeoJSON geometry",
                r#"Attribute `locObj` dropped: `{"type":"Point"}` is not a GeoJSON geometry"#,
                "Attribute `locText` dropped: `not geojson` is not a GeoJSON geometry",
                r#"Attribute `temperature.where` dropped: `{"type":"Point"}` is not a GeoJSON geometry"#,
            ],
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 4, "{store:?}");
    }
}

#[test]
fn a_geo_property_over_a_missing_null_or_blank_value_is_absent_without_a_warning() {
    for (store, record) in STORES.into_iter().flat_map(|store| {
        [
            (store, json!({ "id": "A", "t": 20 })),
            (store, json!({ "id": "A", "t": 20, "gt": null, "go": null, "gn": null })),
            (store, json!({ "id": "A", "t": 20, "gt": "", "go": "  ", "gn": "" })),
        ]
    }) {
        let outcome = run_thing(store, &record);

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Thing:A"], "{store:?} {record}");
        let entity = &outcome.entities[0];
        for absent in ["locText", "locObj", "locNum"] {
            assert!(entity.get(absent).is_none(), "{store:?} {record} {absent}: {entity:?}");
        }
        assert_eq!(entity["temperature"]["value"], "20", "{store:?} {record}: {entity:?}");
        assert!(entity["temperature"].get("where").is_none(), "{store:?} {record}: {entity:?}");
        assert_eq!(outcome.warnings, 0, "{store:?} {record}: {:?}", outcome.diagnostics);
    }
}

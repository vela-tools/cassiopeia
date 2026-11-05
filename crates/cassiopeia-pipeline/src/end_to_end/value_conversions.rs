use crate::end_to_end::harness::{RunBinding, RunInput, STORES, entity_ids, run_end_to_end};
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A counter whose count is read as an integer, beside a name that is always text.
const COUNTER: &str = r#"{
    version: "v4",
    dataModel: "Counter",
    identity: { entityName: "{{ id }}" },
    attributes: {
        name: { source: "{{ name }}" },
        count: { source: "{{ count }}", transformation: "integer" },
    },
}"#;

/// The written entity whose `id` is `id`.
fn entity<'a>(entities: &'a [Value], id: &str) -> &'a Value {
    entities
        .iter()
        .find(|entity| entity["id"] == id)
        .unwrap_or_else(|| panic!("no entity {id} among {entities:?}"))
}

#[test]
fn an_unreadable_integer_is_dropped_with_a_warning_naming_the_attribute_and_the_value() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(COUNTER),
                records: json!([
                    { "id": "a", "name": "north gate", "count": "abc" },
                    { "id": "b", "name": "south gate", "count": " 12 " },
                ]),
            }],
            &["Counter"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Counter:a", "urn:ngsi-ld:Counter:b"], "{store:?}");
        let north = entity(&outcome.entities, "urn:ngsi-ld:Counter:a");
        let south = entity(&outcome.entities, "urn:ngsi-ld:Counter:b");
        assert!(north.get("count").is_none(), "{store:?}: {north:?}");
        assert_eq!(north["name"]["value"], "north gate", "{store:?}");
        assert_eq!(south["count"]["value"], 12, "{store:?}");
        let unreadable = outcome.coded(DiagnosticCode::Extractor(ExtractorCode::NumberUnreadable));
        assert_eq!(unreadable.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(unreadable[0].headline, "Attribute `count` dropped: `abc` is not an integer", "{store:?}");
        assert_eq!(unreadable[0].occurrences, 1, "{store:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

/// A museum whose name is one object of text keyed by language tag, read with no transformation.
const MUSEUM: &str = r#"{
    version: "v4",
    dataModel: "Museum",
    identity: { entityName: "{{ id }}" },
    attributes: {
        name: { type: "LanguageProperty", source: "{{ name }}" },
    },
}"#;

#[test]
fn a_language_property_with_one_source_reads_its_map_by_default() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(MUSEUM),
                records: json!([
                    { "id": "louvre", "name": { "fr": "Musée du Louvre", "en": "Louvre Museum" } },
                    { "id": "rijks", "name": r#"{"nl":"Rijksmuseum"}"# },
                ]),
            }],
            &["Museum"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Museum:louvre", "urn:ngsi-ld:Museum:rijks"], "{store:?}");
        let louvre = entity(&outcome.entities, "urn:ngsi-ld:Museum:louvre");
        let rijks = entity(&outcome.entities, "urn:ngsi-ld:Museum:rijks");
        assert_eq!(louvre["name"]["type"], "LanguageProperty", "{store:?}: {louvre:?}");
        assert_eq!(
            louvre["name"]["languageMap"],
            json!({ "fr": "Musée du Louvre", "en": "Louvre Museum" }),
            "{store:?}"
        );
        assert_eq!(rijks["name"]["languageMap"], json!({ "nl": "Rijksmuseum" }), "{store:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}: {:?}", outcome.diagnostics);
    }
}

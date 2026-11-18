use crate::end_to_end::harness::{RunBinding, STORES, attempt_inputs_end_to_end, entity_ids, run_csv_end_to_end};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, mapping_code::MappingCode};
use pretty_assertions::assert_eq;
use std::fs;
use temp_dir::TempDir;

/// One sensor's reading, with the building it stands in and its maker.
const SENSORS: &str = "sensor,t,building,maker
T1,21.5,B-12,Acme
";

/// A sensors mapping holding keys that loading it ignores: a misspelled `transformation`, a target's
/// `sourceField` and `idPattern`, and a `syntheticEntity` on a Property.
const SENSOR_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Device",
    identity: { entityName: "{{ sensor }}" },
    attributes: {
        temperature: { source: "{{ t }}", transformaton: "float" },
        refBuilding: {
            type: "Relationship",
            source: "{{ building }}",
            target: { entity: "Building", sourceField: "building_uri", idPattern: "B-\\d+" },
        },
        manufacturer: {
            source: "{{ maker }}",
            syntheticEntity: {
                dataModel: "Organization",
                identity: { entityName: "{{ maker }}" },
                attributes: { name: { source: "{{ maker }}" } },
            },
        },
    },
}"#;

const UNKNOWN_KEY: DiagnosticCode = DiagnosticCode::Mapping(MappingCode::V4UnknownKey);
const KEY_WITHOUT_EFFECT: DiagnosticCode = DiagnosticCode::Mapping(MappingCode::V4KeyWithoutEffect);

#[test]
fn the_keys_a_mapping_declares_without_effect_warn_before_any_record_and_the_mapping_still_runs() {
    for store in STORES {
        let outcome = run_csv_end_to_end(store, SENSORS, &RunBinding::Mapping(SENSOR_MAPPING), &["Device"], "");

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Device:T1"], "{store:?}");
        let sensor = &outcome.entities[0];
        assert_eq!(sensor["temperature"]["value"], "21.5", "{store:?}: {sensor}");
        assert_eq!(sensor["refBuilding"]["object"], "urn:ngsi-ld:Building:B-12", "{store:?}: {sensor}");
        assert_eq!(sensor["manufacturer"]["value"], "Acme", "{store:?}: {sensor}");

        assert_eq!(
            outcome.diagnostics.iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>(),
            [UNKNOWN_KEY, KEY_WITHOUT_EFFECT, KEY_WITHOUT_EFFECT, KEY_WITHOUT_EFFECT],
            "{store:?}: {:?}",
            outcome.diagnostics
        );
        let declared = outcome
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.causes[0].split_once(", which").map_or("", |(declared, _)| declared))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            [
                "`temperature` declares `transformaton`",
                "The target of `refBuilding` declares `sourceField`",
                "The target of `refBuilding` declares `idPattern`",
                "`manufacturer` declares a `syntheticEntity`",
            ],
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 4, "{store:?}");
    }
}

#[test]
fn a_mapping_two_inputs_share_warns_once() {
    let directory = TempDir::new().unwrap();
    let mapping = directory.path().join("sensors.json5");
    fs::write(&mapping, SENSOR_MAPPING).unwrap();
    let declared: Vec<String> = ["T1,21.5,B-12,Acme", "T2,19.0,B-14,Acme"]
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let data = directory.path().join(format!("sensors-{index}.csv"));
            fs::write(&data, format!("sensor,t,building,maker\n{row}\n")).unwrap();
            format!(r#"{{ source: "{}", mapping: "{}" }}"#, data.display(), mapping.display())
        })
        .collect();

    let (result, outcome) = attempt_inputs_end_to_end(StoreKind::DashMap, &declared, &["Device"]);

    result.unwrap();
    assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Device:T1", "urn:ngsi-ld:Device:T2"]);
    assert_eq!(outcome.coded(UNKNOWN_KEY).len(), 1, "{:?}", outcome.diagnostics);
    assert_eq!(outcome.coded(KEY_WITHOUT_EFFECT).len(), 3, "{:?}", outcome.diagnostics);
    assert_eq!(outcome.warnings, 4);
}

use crate::end_to_end::harness::{RunBinding, STORES, entity_ids, run_csv_end_to_end};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, transform_code::TransformCode},
    context_field::ContextField,
    unit_text::UnitText,
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use pretty_assertions::assert_eq;
use serde_json::Value;

/// A station's `pm10`, whose `unitCode` translates the unit notation each row carries.
const STATION_UNITS: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "Station-{{ id }}" },
    attributes: {
        pm10: {
            source: "{{ pm10 }}",
            transformation: "float",
            properties: { unitCode: { source: "{{ unit | cefact_unit }}" } },
        },
    },
}"#;

/// Four stations: two units the `cefact_unit` filter translates, and one it does not know, twice.
const STATIONS: &str = "id,pm10,unit\n1,10,ug/m3\n2,11,ppm\n3,12,bogus\n4,13,bogus\n";

#[test]
fn csv_units_are_translated_and_unknown_ones_reported() {
    for store in STORES {
        let outcome = run_csv_end_to_end(store, STATIONS, &RunBinding::Mapping(STATION_UNITS), &["Sensor"]);

        assert_eq!(
            entity_ids(&outcome),
            [
                "urn:ngsi-ld:Sensor:Station-1",
                "urn:ngsi-ld:Sensor:Station-2",
                "urn:ngsi-ld:Sensor:Station-3",
                "urn:ngsi-ld:Sensor:Station-4",
            ],
            "{store:?}"
        );
        let pm10 = |station: &str| -> &Value {
            let id = format!("urn:ngsi-ld:Sensor:Station-{station}");
            let entity = outcome.entities.iter().find(|entity| entity["id"] == id.as_str()).expect("station written");
            &entity["pm10"]
        };
        assert_eq!(pm10("1")["unitCode"], "GQ", "{store:?}");
        assert_eq!(pm10("2")["unitCode"], "59", "{store:?}");
        for station in ["3", "4"] {
            assert!(pm10(station).get("value").is_some(), "{store:?} {station}: {:?}", pm10(station));
            assert!(pm10(station).get("unitCode").is_none(), "{store:?} {station}: {:?}", pm10(station));
        }

        let unknown = outcome.coded(DiagnosticCode::Transform(TransformCode::UnitCodeUnknown));
        assert_eq!(unknown.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(unknown[0].occurrences, 2, "{store:?}");
        assert_eq!(
            unknown[0].defining,
            [
                ContextField::Attribute(AttributePath::top_level(NameBuf::new("pm10").unwrap())),
                ContextField::UnitText(UnitText::new("bogus")),
            ],
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 2, "{store:?}: {:?}", outcome.diagnostics);
    }
}

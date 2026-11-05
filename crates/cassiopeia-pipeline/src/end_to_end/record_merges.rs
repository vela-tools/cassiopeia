use crate::end_to_end::{
    harness::{RunBinding, RunInput, STORES, entity_ids, run_end_to_end},
    station_mappings::{STATION_NAME, STATION_READINGS, STATION_READINGS_AND_LEVEL, STATION_TEMPERATURE},
};
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, resolver_code::ResolverCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The code a record-merge warning is published under.
const RECORDS_MERGED: DiagnosticCode = DiagnosticCode::Resolver(ResolverCode::RecordsMerged);

const CONSTANT_WEATHER_STATION: &str = r#"{
    version: "v4",
    dataModel: "WeatherStation",
    identity: { entityName: "WeatherStation" },
    attributes: { temperature: { source: "{{ temperature }}" } },
}"#;

const MOUNTAIN_WITH_SYNTHETIC_COUNTRY: &str = r#"{
    version: "v4",
    dataModel: "Mountain",
    identity: { entityName: "{{ name }}" },
    attributes: {
        height: { source: "{{ height }}" },
        hasCountry: {
            source: "{{ country }}",
            type: "Relationship",
            target: { entity: "Country" },
            syntheticEntity: {
                dataModel: "Country",
                identity: { entityName: "{{ country }}" },
                attributes: { name: { source: "{{ country }}" } },
            },
        },
    },
}"#;

const CONSTANT_GOLD_PRICE: &str = r#"{
    version: "v4",
    dataModel: "GoldPriceObserved",
    identity: { entityName: "gold" },
    attributes: {
        price: {
            source: "{{ price }}",
            properties: { observedAt: { source: "{{ date }}" } },
        },
    },
}"#;

#[test]
fn two_differing_readings_under_one_constant_identity_become_one_entity_and_exactly_one_merge_warning() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(CONSTANT_WEATHER_STATION),
                records: json!([{ "temperature": 20 }, { "temperature": 21 }]),
            }],
            &["WeatherStation"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:WeatherStation:WeatherStation"], "{store:?}");
        assert_eq!(outcome.coded(RECORDS_MERGED).len(), 1, "{store:?}");
        assert_eq!(
            outcome.coded(RECORDS_MERGED)[0].headline,
            "2 records resolved to `urn:ngsi-ld:WeatherStation:WeatherStation` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded",
            "{store:?}"
        );
        assert_eq!(outcome.coded(RECORDS_MERGED)[0].occurrences, 1, "{store:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn records_disagreeing_on_one_id_warn_once_for_that_id_and_never_for_an_id_seen_once() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_TEMPERATURE),
                records: json!([
                    { "id": "a", "temperature": 20 },
                    { "id": "a", "temperature": 21 },
                    { "id": "b", "temperature": 22 },
                ]),
            }],
            &["Station"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:a", "urn:ngsi-ld:Station:b"], "{store:?}");
        assert_eq!(outcome.coded(RECORDS_MERGED).len(), 1, "{store:?}");
        assert_eq!(
            outcome.coded(RECORDS_MERGED)[0].headline,
            "2 records resolved to `urn:ngsi-ld:Station:a` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded",
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn identical_records_repeating_one_id_do_not_warn() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_TEMPERATURE),
                records: json!([{ "id": "a", "temperature": 20 }, { "id": "a", "temperature": 20 }]),
            }],
            &["Station"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:a"], "{store:?}");
        assert!(outcome.coded(RECORDS_MERGED).is_empty(), "{store:?}: {:?}", outcome.coded(RECORDS_MERGED));
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn records_that_each_add_their_own_reading_to_one_id_do_not_warn() {
    let geometry = json!({ "type": "Point", "coordinates": [14.5, 46.05] });
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_READINGS),
                records: json!([
                    { "id": "cell", "geometry": geometry, "temperature": 20.5 },
                    { "id": "cell", "geometry": geometry, "wind": 3.5 },
                ]),
            }],
            &["Station"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:cell"], "{store:?}");
        assert!(
            outcome.entities[0].get("temperature").is_some() && outcome.entities[0].get("windSpeed").is_some(),
            "{store:?}"
        );
        assert!(outcome.coded(RECORDS_MERGED).is_empty(), "{store:?}: {:?}", outcome.coded(RECORDS_MERGED));
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

/// Two per-level slices of one grid cell, the way a GRIB source pivots its parameters: each
/// carries its own reading and its own vertical level.
fn grid_cell_slices() -> Value {
    json!([
        { "id": "cell", "level_type": "height_above_ground", "level": 2, "temperature": 280.1 },
        { "id": "cell", "level_type": "height_above_ground", "level": 10, "wind": 3.5 },
    ])
}

#[test]
fn per_level_slices_of_one_cell_disagreeing_only_on_a_level_the_mapping_never_reads_do_not_warn() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_READINGS),
                records: grid_cell_slices(),
            }],
            &["Station"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:cell"], "{store:?}");
        assert!(outcome.coded(RECORDS_MERGED).is_empty(), "{store:?}: {:?}", outcome.coded(RECORDS_MERGED));
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn per_level_slices_of_one_cell_warn_naming_the_level_once_the_mapping_reads_it() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_READINGS_AND_LEVEL),
                records: grid_cell_slices(),
            }],
            &["Station"],
            "",
        );

        assert_eq!(outcome.coded(RECORDS_MERGED).len(), 1, "{store:?}");
        assert_eq!(
            outcome.coded(RECORDS_MERGED)[0].headline,
            "2 records resolved to `urn:ngsi-ld:Station:cell` and disagreed on `level`; one value of each disagreeing field was kept and the others were discarded",
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn records_placing_one_id_at_different_coordinates_warn_naming_the_coordinates() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_READINGS),
                records: json!([
                    { "id": "shop", "geometry": { "type": "Point", "coordinates": [-73.98, 40.78] } },
                    { "id": "shop", "geometry": { "type": "Point", "coordinates": [-74.0, 40.71] } },
                ]),
            }],
            &["Station"],
            "",
        );

        assert_eq!(outcome.coded(RECORDS_MERGED).len(), 1, "{store:?}");
        assert_eq!(
            outcome.coded(RECORDS_MERGED)[0].headline,
            "2 records resolved to `urn:ngsi-ld:Station:shop` and disagreed on `geometry.coordinates`; one value of each disagreeing field was kept and the others were discarded",
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn two_mappings_joined_onto_one_id_do_not_warn() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[
                RunInput {
                    binding: RunBinding::Mapping(STATION_TEMPERATURE),
                    records: json!([{ "id": "a", "temperature": 20 }]),
                },
                RunInput {
                    binding: RunBinding::Mapping(STATION_NAME),
                    records: json!([{ "id": "a", "name": "Bežigrad" }]),
                },
            ],
            &["Station"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:a"], "{store:?}");
        assert!(
            outcome.entities[0].get("temperature").is_some() && outcome.entities[0].get("name").is_some(),
            "{store:?}"
        );
        assert!(outcome.coded(RECORDS_MERGED).is_empty(), "{store:?}: {:?}", outcome.coded(RECORDS_MERGED));
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_synthetic_entity_named_by_many_records_does_not_warn() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(MOUNTAIN_WITH_SYNTHETIC_COUNTRY),
                records: json!([
                    { "name": "Everest", "height": 8849, "country": "Nepal" },
                    { "name": "Lhotse", "height": 8516, "country": "Nepal" },
                    { "name": "Makalu", "height": 8485, "country": "Nepal" },
                ]),
            }],
            &["Mountain", "Country"],
            "",
        );

        assert_eq!(
            entity_ids(&outcome),
            [
                "urn:ngsi-ld:Country:Nepal",
                "urn:ngsi-ld:Mountain:Everest",
                "urn:ngsi-ld:Mountain:Lhotse",
                "urn:ngsi-ld:Mountain:Makalu",
            ],
            "{store:?}"
        );
        assert!(outcome.coded(RECORDS_MERGED).is_empty(), "{store:?}: {:?}", outcome.coded(RECORDS_MERGED));
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_temporal_mapping_feeding_one_entity_from_several_records_does_not_warn() {
    let records = json!([
        { "price": 1900.5, "date": "2026-01-01T00:00:00Z" },
        { "price": 1950.25, "date": "2026-02-01T00:00:00Z" },
        { "price": 2010.0, "date": "2026-03-01T00:00:00Z" },
    ]);
    for (store, output_extra) in STORES
        .into_iter()
        .flat_map(|store| [(store, ""), (store, r#", temporal: { representation: "series" }"#)])
    {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(CONSTANT_GOLD_PRICE),
                records: records.clone(),
            }],
            &["GoldPriceObserved"],
            output_extra,
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:GoldPriceObserved:gold"], "{store:?} {output_extra}");
        assert!(
            outcome.coded(RECORDS_MERGED).is_empty(),
            "{store:?} {output_extra}: {:?}",
            outcome.coded(RECORDS_MERGED)
        );
        assert_eq!(outcome.warnings, 0, "{store:?} {output_extra}");
    }
}

#[test]
fn a_static_mapping_disagreeing_on_an_id_in_a_series_run_warns_too() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_TEMPERATURE),
                records: json!([{ "id": "a", "temperature": 20 }, { "id": "a", "temperature": 21 }]),
            }],
            &["Station"],
            r#", temporal: { representation: "series" }"#,
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:a"], "{store:?}");
        assert_eq!(outcome.coded(RECORDS_MERGED).len(), 1, "{store:?}");
        assert_eq!(
            outcome.coded(RECORDS_MERGED)[0].headline,
            "2 records resolved to `urn:ngsi-ld:Station:a` and disagreed on `temperature`; one value of each disagreeing field was kept and the others were discarded",
            "{store:?}"
        );
    }
}

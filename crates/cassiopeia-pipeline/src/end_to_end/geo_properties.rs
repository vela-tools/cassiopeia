use crate::end_to_end::{
    harness::{RunBinding, RunInput, RunOutcome, STORES, entity_ids, run_end_to_end},
    station_mappings::STATION_READINGS,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, geometry_code::GeometryCode};
use pretty_assertions::{assert_eq, assert_ne};
use serde_json::{Value, json};

/// A `GeoProperty` declared with no `transformation`, whose `geometry` block asks for the
/// centroid of the geometry the source carries.
const ZONE_CENTROID: &str = r#"{
    version: "v4",
    dataModel: "Zone",
    identity: { entityName: "{{ id }}" },
    attributes: {
        location: { type: "GeoProperty", source: "{{ geometry }}", geometry: { convert: "centroid" } },
    },
}"#;

#[test]
fn a_geo_property_without_a_transformation_publishes_the_geometry_its_geometry_block_derives() {
    let square = json!({ "type": "Polygon", "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]] });
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(ZONE_CENTROID),
                records: json!([{ "id": "square", "geometry": square }]),
            }],
            &["Zone"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Zone:square"], "{store:?}");
        assert_eq!(
            outcome.entities[0]["location"],
            json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 1.0] } }),
            "{store:?}"
        );
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

/// A clockwise square: an exterior ring wound against the right-hand rule RFC 7946 clause 3.1.6
/// asks for.
fn clockwise_square() -> Value {
    json!({ "type": "Polygon", "coordinates": [[[0.0, 0.0], [0.0, 2.0], [2.0, 2.0], [2.0, 0.0], [0.0, 0.0]]] })
}

/// Runs one zone through `mapping` and returns its published `location` value.
fn published_zone_location(store: StoreKind, mapping: &'static str) -> Value {
    let mut outcome = run_end_to_end(
        store,
        &[RunInput {
            binding: RunBinding::Mapping(mapping),
            records: json!([{ "id": "square", "geometry": clockwise_square() }]),
        }],
        &["Zone"],
        "",
    );

    outcome.entities[0]["location"]["value"].take()
}

#[test]
fn a_geo_property_keeping_its_winding_publishes_the_ring_as_the_source_wound_it() {
    let keep = r#"{
        version: "v4",
        dataModel: "Zone",
        identity: { entityName: "{{ id }}" },
        attributes: {
            location: { type: "GeoProperty", transformation: "polygon", source: "{{ geometry }}", geometry: { winding: "keep" } },
        },
    }"#;
    for store in STORES {
        assert_eq!(published_zone_location(store, keep), clockwise_square(), "{store:?}");
    }
}

#[test]
fn a_geo_property_without_a_winding_policy_publishes_the_ring_rewound() {
    let rewind = r#"{
        version: "v4",
        dataModel: "Zone",
        identity: { entityName: "{{ id }}" },
        attributes: {
            location: { type: "GeoProperty", transformation: "polygon", source: "{{ geometry }}" },
        },
    }"#;
    for store in STORES {
        assert_ne!(published_zone_location(store, rewind), clockwise_square(), "{store:?}");
    }
}

#[test]
fn a_geometry_collection_under_a_geo_property_without_a_transformation_is_dropped_with_a_warning() {
    let collection = json!({ "type": "GeometryCollection", "geometries": [{ "type": "Point", "coordinates": [1.0, 2.0] }] });
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_READINGS),
                records: json!([{ "id": "cell", "geometry": collection, "temperature": 20.5 }]),
            }],
            &["Station"],
            "",
        );

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Station:cell"], "{store:?}");
        assert!(outcome.entities[0].get("location").is_none(), "{store:?}: {:?}", outcome.entities[0]);
        assert!(outcome.entities[0].get("temperature").is_some(), "{store:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

/// A zone whose `name` carries its `footprint` as a `GeoProperty` sub-attribute, with
/// `geometry_block` spliced into the sub-attribute's declaration.
fn zone_with_footprint(geometry_block: &str) -> &'static str {
    Box::leak(
        format!(
            r#"{{
                version: "v4",
                dataModel: "Zone",
                identity: {{ entityName: "{{{{ id }}}}" }},
                attributes: {{
                    name: {{
                        source: "{{{{ id }}}}",
                        properties: {{ footprint: {{ type: "GeoProperty", source: "{{{{ geometry }}}}"{geometry_block} }} }},
                    }},
                }},
            }}"#
        )
        .into_boxed_str(),
    )
}

/// Runs one zone carrying `geometry` through a mapping declaring it as the `footprint`
/// sub-attribute of `name`, under `geometry_block`.
fn run_zone_footprint(store: StoreKind, geometry_block: &str, geometry: &Value) -> RunOutcome {
    run_end_to_end(
        store,
        &[RunInput {
            binding: RunBinding::Mapping(zone_with_footprint(geometry_block)),
            records: json!([{ "id": "square", "geometry": geometry }]),
        }],
        &["Zone"],
        "",
    )
}

#[test]
fn a_geo_property_sub_attribute_keeping_its_winding_publishes_the_ring_as_the_source_wound_it() {
    for store in STORES {
        let outcome = run_zone_footprint(store, r#", geometry: { winding: "keep" }"#, &clockwise_square());

        assert_eq!(outcome.entities[0]["name"]["footprint"]["value"], clockwise_square(), "{store:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_geo_property_sub_attribute_without_a_winding_policy_publishes_the_ring_rewound() {
    for store in STORES {
        let outcome = run_zone_footprint(store, "", &clockwise_square());

        let footprint = &outcome.entities[0]["name"]["footprint"];
        assert_eq!(footprint["type"], "GeoProperty", "{store:?}");
        assert_ne!(footprint["value"], clockwise_square(), "{store:?}");
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_geometry_collection_under_a_geo_property_sub_attribute_is_dropped_with_a_warning_and_the_parent_still_published() {
    let collection = json!({ "type": "GeometryCollection", "geometries": [{ "type": "Point", "coordinates": [1.0, 2.0] }] });
    for store in STORES {
        let outcome = run_zone_footprint(store, "", &collection);

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Zone:square"], "{store:?}");
        let name = &outcome.entities[0]["name"];
        assert_eq!(name["value"], "square", "{store:?}");
        assert!(name.get("footprint").is_none(), "{store:?}: {name:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn a_geo_property_sub_attribute_over_null_or_blank_text_is_absent_without_a_warning() {
    for (store, geometry) in STORES.into_iter().flat_map(|store| [(store, json!("")), (store, Value::Null)]) {
        let outcome = run_zone_footprint(store, "", &geometry);

        let name = &outcome.entities[0]["name"];
        assert_eq!(name["value"], "square", "{store:?} {geometry}");
        assert!(name.get("footprint").is_none(), "{store:?} {geometry}: {name:?}");
        assert_eq!(outcome.warnings, 0, "{store:?} {geometry}");
    }
}

#[test]
fn a_geo_property_sub_attribute_over_a_value_that_is_no_geometry_is_dropped_with_a_warning_and_the_parent_still_published() {
    for (store, geometry) in STORES.into_iter().flat_map(|store| {
        [
            (store, json!("somewhere near the river")),
            (store, json!({ "type": "Point" })),
            (store, json!(42)),
        ]
    }) {
        let outcome = run_zone_footprint(store, "", &geometry);

        assert_eq!(entity_ids(&outcome), ["urn:ngsi-ld:Zone:square"], "{store:?} {geometry}");
        let name = &outcome.entities[0]["name"];
        assert_eq!(name["value"], "square", "{store:?} {geometry}");
        assert!(name.get("footprint").is_none(), "{store:?} {geometry}: {name:?}");
        assert_eq!(outcome.warnings, 1, "{store:?} {geometry}");
    }
}

#[test]
fn a_refused_geometry_in_one_instance_of_a_multi_instance_geo_property_is_dropped_with_a_warning() {
    let mapping = r#"{
        version: "v4",
        dataModel: "Zone",
        identity: { entityName: "{{ id }}" },
        attributes: {
            location: {
                type: "GeoProperty",
                instances: [
                    { source: "{{ surveyed }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:surveyed" } } },
                    { source: "{{ estimated }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:estimated" } } },
                ],
            },
        },
    }"#;
    let collection = json!({ "type": "GeometryCollection", "geometries": [{ "type": "Point", "coordinates": [1.0, 2.0] }] });
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(mapping),
                records: json!([{ "id": "square", "surveyed": { "type": "Point", "coordinates": [1.0, 1.0] }, "estimated": collection }]),
            }],
            &["Zone"],
            "",
        );

        let location = &outcome.entities[0]["location"];
        assert_eq!(location.as_array().map(Vec::len), Some(1), "{store:?}: {location:?}");
        assert_eq!(location[0]["type"], "GeoProperty", "{store:?}: {location:?}");
        assert_eq!(location[0]["datasetId"], "urn:ngsi-ld:Dataset:surveyed", "{store:?}: {location:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn geometries_outside_wgs_84_are_dropped_with_one_warning_naming_the_first_position_and_the_entities_still_published() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION_READINGS),
                records: json!([
                    { "id": "projected", "geometry": { "type": "Point", "coordinates": [2600.5, 341_870.2] }, "temperature": 20.5 },
                    { "id": "swapped", "geometry": { "type": "LineString", "coordinates": [[16.37, 48.2], [16.37, 91.0]] }, "temperature": 21.0 },
                    { "id": "here", "geometry": { "type": "Point", "coordinates": [16.37, 48.2, 9000.0] }, "temperature": 19.0 },
                ]),
            }],
            &["Station"],
            "",
        );

        assert_eq!(
            entity_ids(&outcome),
            ["urn:ngsi-ld:Station:here", "urn:ngsi-ld:Station:projected", "urn:ngsi-ld:Station:swapped"],
            "{store:?}"
        );
        let located = outcome
            .entities
            .iter()
            .filter(|entity| entity.get("location").is_some())
            .map(|entity| &entity["id"])
            .collect::<Vec<&Value>>();
        assert_eq!(located, ["urn:ngsi-ld:Station:here"], "{store:?}");
        let refused = outcome.coded(DiagnosticCode::Geometry(GeometryCode::CoordinatesOutOfRange));
        assert_eq!(refused.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(refused[0].occurrences, 2, "{store:?}");
        // The store decides which record of the batch is resolved first, so either refused position may
        // be the example the warning quotes.
        let examples = [
            "Attribute `location` dropped: the position `[2600.5, 341870.2]` at `coordinates` has a longitude outside WGS 84;",
            "Attribute `location` dropped: the position `[16.37, 91]` at `coordinates[1]` has a latitude outside WGS 84;",
        ];
        assert!(
            examples.iter().any(|example| refused[0].headline.starts_with(example)),
            "{store:?}: {}",
            refused[0].headline
        );
        assert_eq!(outcome.warnings, 2, "{store:?}");
    }
}

#[test]
fn a_geo_property_sub_attribute_outside_wgs_84_is_dropped_with_a_warning_naming_its_path_and_the_parent_still_published() {
    let polygon = json!({ "type": "Polygon", "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, -95.0], [0.0, 0.0]]] });
    for store in STORES {
        let outcome = run_zone_footprint(store, "", &polygon);

        assert!(outcome.entities[0]["name"].get("footprint").is_none(), "{store:?}: {:?}", outcome.entities[0]);
        let refused = outcome.coded(DiagnosticCode::Geometry(GeometryCode::CoordinatesOutOfRange));
        assert_eq!(refused.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert!(
            refused[0]
                .headline
                .starts_with("Attribute `name.footprint` dropped: the position `[2, -95]` at `coordinates[0][2]` has a latitude outside WGS 84;"),
            "{store:?}: {}",
            refused[0].headline
        );
    }
}

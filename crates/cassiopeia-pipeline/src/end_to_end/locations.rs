use crate::end_to_end::{
    harness::{RunOutcome, RunSchemas, STORES, attempt_file_end_to_end, entity_ids},
    sensorthings::{entity, fixture},
};
use cassiopeia_common::store_kind::StoreKind;
use pretty_assertions::assert_eq;
use serde_json::json;

/// The stations of the Locations fixture, as they are written: the first has a text Location with a
/// lower id than its `GeoJSON` Location, and the second has one `GeoJSON` Location.
const STATIONS: [&str; 2] = ["urn:ngsi-ld:AirQualityObserved:sta-hexample-1", "urn:ngsi-ld:AirQualityObserved:sta-hexample-2"];

/// Runs the Locations fixture through the mapping that addresses each station by the name of its
/// Location with the lowest id, on `store`, validating against the fixtures' schemas.
fn run(store: StoreKind) -> RunOutcome {
    let (result, outcome) = attempt_file_end_to_end(
        store,
        &fixture("locations-v1.1-latest.jsonl"),
        &fixture("addressed-air-quality-observed.json5"),
        &RunSchemas::Folder(&fixture("schemas")),
        "",
    );
    result.unwrap();
    outcome
}

#[test]
fn each_station_is_placed_at_its_geojson_location_and_addressed_by_its_first_location() {
    for store in STORES {
        let outcome = run(store);

        assert_eq!(entity_ids(&outcome), STATIONS, "{store:?}");
        let [stationsplein, kanaaldijk] = STATIONS.map(|station| entity(&outcome, station));
        assert_eq!(
            stationsplein["location"]["value"],
            json!({"type": "Point", "coordinates": [5.4797, 51.4433]}),
            "{store:?}"
        );
        assert_eq!(
            stationsplein["address"]["value"],
            json!({"streetAddress": "Stationsplein 1, Eindhoven"}),
            "{store:?}"
        );
        assert_eq!(
            kanaaldijk["location"]["value"],
            json!({"type": "Point", "coordinates": [5.64, 51.475]}),
            "{store:?}"
        );
        assert_eq!(kanaaldijk["address"]["value"], json!({"streetAddress": "Kanaaldijk 5, Helmond"}), "{store:?}");
        assert!(outcome.diagnostics.is_empty(), "{store:?}: {:?}", outcome.diagnostics);
    }
}

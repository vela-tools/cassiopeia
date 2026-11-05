use crate::end_to_end::harness::{RunBinding, RunInput, STORES, entity_ids, run_end_to_end};
use pretty_assertions::assert_eq;
use serde_json::json;

/// One air temperature per station, observed at the time its collection states once for all of them.
const AIR_TEMPERATURE: &str = r#"{
    version: "v4",
    dataModel: "WeatherObserved",
    identity: { entityName: "{{ properties.station }}" },
    attributes: {
        temperature: {
            source: "{{ properties.parameters.TL.data | first }}",
            transformation: "float",
            properties: {
                observedAt: { source: "{{ featureCollection.timestamps | first }}", transformation: "datetime" },
            },
        },
    },
}"#;

#[test]
fn a_feature_reads_the_observation_time_its_feature_collection_states_once() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(AIR_TEMPERATURE),
                records: json!({
                    "type": "FeatureCollection",
                    "timestamps": ["2026-10-25T07:50+00:00"],
                    "features": [
                        {
                            "type": "Feature",
                            "geometry": { "type": "Point", "coordinates": [16.37, 48.2] },
                            "properties": { "station": "11034", "parameters": { "TL": { "name": "Lufttemperatur", "unit": "°C", "data": [6.9] } } }
                        },
                        {
                            "type": "Feature",
                            "geometry": { "type": "Point", "coordinates": [16.36, 48.25] },
                            "properties": { "station": "11035", "parameters": { "TL": { "name": "Lufttemperatur", "unit": "°C", "data": [7.4] } } }
                        }
                    ]
                }),
            }],
            &["WeatherObserved"],
            "",
        );

        assert_eq!(
            entity_ids(&outcome),
            ["urn:ngsi-ld:WeatherObserved:11034", "urn:ngsi-ld:WeatherObserved:11035"],
            "{store:?}"
        );
        for entity in &outcome.entities {
            assert_eq!(entity["temperature"]["observedAt"], "2026-10-25T07:50:00Z", "{store:?}: {entity:?}");
        }
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

use crate::end_to_end::{
    harness::{RunOutcome, RunSchemas, STORES, attempt_file_end_to_end, entity_ids},
    sensorthings::{entity, fixture},
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode};
use pretty_assertions::assert_eq;

/// The stations of the Sensors fixture, as they are written: the first reads `pm10` from Sensor 7,
/// the second from Sensor 8, the third reads no `pm10`, and the fourth reads it from a data stream
/// served without a Sensor.
const STATIONS: [&str; 4] = [
    "urn:ngsi-ld:AirQualityObserved:sta-hexample-1",
    "urn:ngsi-ld:AirQualityObserved:sta-hexample-2",
    "urn:ngsi-ld:AirQualityObserved:sta-hexample-3",
    "urn:ngsi-ld:AirQualityObserved:sta-hexample-4",
];

/// The Devices that describe Sensors 7 and 8 of `https://h.example/v1.1`.
const DEVICES: [&str; 2] = ["urn:ngsi-ld:Device:sta-hexample-sensor-7", "urn:ngsi-ld:Device:sta-hexample-sensor-8"];

/// Runs the Sensors fixture through the mapping that links each station to the `Device` of its
/// `pm10` Sensor, on `store`, validating against the fixtures' schemas, which hold none for `Device`.
fn run(store: StoreKind) -> RunOutcome {
    let (result, outcome) = attempt_file_end_to_end(
        store,
        &fixture("sensors-v1.1-latest.jsonl"),
        &fixture("device-air-quality-observed.json5"),
        &RunSchemas::Folder(&fixture("schemas")),
        "",
    );
    result.unwrap();
    outcome
}

#[test]
fn each_station_links_the_device_of_its_pm10_sensor() {
    for store in STORES {
        let outcome = run(store);

        let mut written = STATIONS.to_vec();
        written.extend(DEVICES);
        assert_eq!(entity_ids(&outcome), written, "{store:?}");
        for (station, device) in STATIONS.iter().zip(DEVICES) {
            let link = &entity(&outcome, station)["refDevice"];
            assert_eq!(link["type"], "Relationship", "{store:?} {station}: {link}");
            assert_eq!(link["object"], device, "{store:?} {station}: {link}");
        }
        let devices = DEVICES.map(|device| entity(&outcome, device));
        assert_eq!(devices.map(|device| &device["type"]), ["Device", "Device"], "{store:?}");
        assert_eq!(devices.map(|device| &device["name"]["value"]), ["PM-200", "AQ-Multi"], "{store:?}");
        assert_eq!(
            devices.map(|device| &device["description"]["value"]),
            ["Optical particulate matter sensor", "Multi-pollutant air quality monitor"],
            "{store:?}"
        );
    }
}

#[test]
fn a_station_without_a_pm10_sensor_links_no_device_and_writes_none() {
    for store in STORES {
        let outcome = run(store);

        let [.., without_pm10, without_sensor] = STATIONS;
        assert_eq!(entity(&outcome, without_pm10)["no2"]["value"], 31.0, "{store:?}");
        assert_eq!(entity(&outcome, without_sensor)["pm10"]["value"], 12.7, "{store:?}");
        for station in [without_pm10, without_sensor] {
            let written = entity(&outcome, station);
            assert!(written.get("refDevice").is_none(), "{store:?} {station}: {written}");
        }
        let devices: Vec<&str> = outcome
            .entities
            .iter()
            .filter(|entity| entity["type"] == "Device")
            .filter_map(|entity| entity["id"].as_str())
            .collect();
        assert_eq!(devices.len(), DEVICES.len(), "{store:?}: {devices:?}");
    }
}

#[test]
fn devices_without_a_schema_are_written_with_one_warning() {
    let outcome = run(StoreKind::DashMap);

    let absent = outcome.coded(DiagnosticCode::Schema(SchemaCode::Absent));
    assert_eq!(absent.len(), 1, "{:?}", outcome.diagnostics);
    assert!(absent[0].headline.contains("'Device'"), "{}", absent[0].headline);
    assert_eq!(outcome.diagnostics.len(), 1, "{:?}", outcome.diagnostics);
}

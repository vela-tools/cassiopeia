use crate::{
    end_to_end::{
        harness::{RunOutcome, RunSchemas, STORES, attempt_file_end_to_end, entity_ids},
        sensorthings::{SERIES_OUTPUT, entity, fixture, instant_times},
    },
    error::PipelineError,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path};
use temp_dir::TempDir;

/// The entity the bus of the snapshots is written as.
const BUS: &str = "urn:ngsi-ld:AirQualityObserved:bus-12";

/// A range snapshot of a SensorThings server with one moving Thing, `Bus 12`, two Observations of
/// its `pm10` Datastream and two `HistoricalLocations`.
const BUS_SNAPSHOT: &str = r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"range","collectedAt":"2026-11-16T09:00:00Z"}
{"datastream":{"@iot.id":7,"name":"bus-12 pm10","unitOfMeasurement":{"symbol":"ug/m3"},"Thing":{"@iot.id":12,"name":"Bus 12","Locations":[{"@iot.id":103,"encodingType":"application/geo+json","location":{"type":"Point","coordinates":[5.487,51.4381]}}]},"ObservedProperty":{"@iot.id":1,"name":"pm10"}}}
{"observation":{"@iot.id":701,"phenomenonTime":"2026-11-16T08:00:00Z","result":18.2},"datastream":7}
{"observation":{"@iot.id":702,"phenomenonTime":"2026-11-16T08:10:00Z","result":21.5},"datastream":7}
{"historicalLocation":{"@iot.id":31,"time":"2026-11-16T07:58:00Z","Locations":[{"@iot.id":101,"encodingType":"application/geo+json","location":{"type":"Point","coordinates":[5.4797,51.4433]}}]},"thing":12}
{"historicalLocation":{"@iot.id":32,"time":"2026-11-16T08:08:00Z","Locations":[{"@iot.id":102,"encodingType":"application/geo+json","location":{"type":"Point","coordinates":[5.4812,51.4402]}}]},"thing":12}
"#;

/// The mapping that writes the bus's track as `location` instances, read from `location`, beside its
/// `pm10` readings, read with the `pm10` members `pm10_reading`.
fn track_mapping(location: &str, pm10_reading: &str) -> String {
    format!(
        r#"{{
            version: "v4",
            dataModel: "dataModel.Environment/AirQualityObserved",
            identity: {{ entityName: "bus-{{{{ thing.id }}}}" }},
            attributes: {{
                location: {{
                    type: "GeoProperty",
                    source: "{location}",
                    properties: {{
                        observedAt: {{ source: "{{{{ historicalLocation.time }}}}", transformation: "datetime" }},
                    }},
                }},
                dateObserved: {{ source: "{{{{ readings.pm10.time }}}}", transformation: "datetime" }},
                pm10: {{
                    source: "{{{{ readings.pm10.result }}}}",
                    {pm10_reading}
                    properties: {{
                        observedAt: {{ source: "{{{{ readings.pm10.time }}}}", transformation: "datetime" }},
                    }},
                }},
            }},
        }}"#
    )
}

/// The track mapping as the source formats guide shows it.
fn guide_mapping() -> String {
    track_mapping("{{ historicalLocation.location }}", r#"transformation: "float","#)
}

/// The track mapping with `location` read from a member no record holds.
fn placeless_mapping() -> String {
    track_mapping("{{ historicalLocation.place }}", r#"transformation: "float","#)
}

/// Runs `snapshot` through `mapping` on `store` into series files in `folder`, with `output_extra`
/// spliced into the output, validating against the published `AirQualityObserved` schema.
fn run(store: StoreKind, folder: &Path, snapshot: &str, mapping: &str, output_extra: &str) -> (Result<(), PipelineError>, RunOutcome) {
    let source = folder.join("bus.jsonl");
    fs::write(&source, snapshot).unwrap();
    let mapping_file = folder.join("track.json5");
    fs::write(&mapping_file, mapping).unwrap();
    attempt_file_end_to_end(
        store,
        &source,
        &mapping_file,
        &RunSchemas::Folder(&fixture("schemas")),
        &format!("{SERIES_OUTPUT}{output_extra}"),
    )
}

/// The message of the error a run ended with.
fn failure(result: Result<(), PipelineError>) -> String {
    let Err(error) = result else {
        panic!("the run was expected to fail");
    };
    error.to_string()
}

/// The value of every instance of the series attribute `attribute`, in the order written.
fn values(attribute: &Value) -> Vec<&Value> {
    attribute
        .as_array()
        .expect("a series attribute is an array of instances")
        .iter()
        .map(|instance| &instance["value"])
        .collect()
}

#[test]
fn a_series_run_checks_a_required_member_on_the_entity_it_writes() {
    for store in STORES {
        let folder = TempDir::new().unwrap();

        let (result, outcome) = run(store, folder.path(), BUS_SNAPSHOT, &guide_mapping(), "");
        result.unwrap();

        assert_eq!(entity_ids(&outcome), [BUS], "{store:?}");
        let bus = entity(&outcome, BUS);
        assert_eq!(
            values(&bus["location"]),
            [
                &json!({"type": "Point", "coordinates": [5.4797, 51.4433]}),
                &json!({"type": "Point", "coordinates": [5.4812, 51.4402]})
            ],
            "{store:?}"
        );
        assert_eq!(instant_times(&bus["location"]), ["2026-11-16T07:58:00Z", "2026-11-16T08:08:00Z"], "{store:?}");
        assert_eq!(values(&bus["pm10"]), [&json!(18.2), &json!(21.5)], "{store:?}");
        assert!(bus.get("dateObserved").is_some(), "{store:?}: {bus}");
        assert!(outcome.diagnostics.is_empty(), "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(outcome.warnings, 0, "{store:?}");
    }
}

#[test]
fn a_required_member_no_record_holds_aborts_the_run_once_naming_the_entity() {
    for store in STORES {
        let folder = TempDir::new().unwrap();

        let (result, outcome) = run(store, folder.path(), BUS_SNAPSHOT, &placeless_mapping(), "");

        assert_eq!(
            failure(result),
            r#"Entity urn:ngsi-ld:AirQualityObserved:bus-12 of type 'AirQualityObserved' does not conform to its schema: 1 violation(s), first at '': "location" is a required property"#,
            "{store:?}"
        );
        assert_eq!(outcome.entities, Vec::<Value>::new(), "{store:?}");
        assert_eq!(outcome.coded(DiagnosticCode::Schema(SchemaCode::Nonconformant)).len(), 0, "{store:?}");
    }
}

#[test]
fn a_required_member_no_record_holds_is_one_warning_for_the_entity_when_relaxed() {
    for store in STORES {
        let folder = TempDir::new().unwrap();

        let (result, outcome) = run(store, folder.path(), BUS_SNAPSHOT, &placeless_mapping(), r#", validation: { mode: "warn" }"#);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), [BUS], "{store:?}");
        let bus = entity(&outcome, BUS);
        assert_eq!(bus.get("location"), None, "{store:?}");
        assert_eq!(values(&bus["pm10"]), [&json!(18.2), &json!(21.5)], "{store:?}");
        let warned = outcome.coded(DiagnosticCode::Schema(SchemaCode::Nonconformant));
        assert_eq!(warned.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(
            warned[0].headline, "1 'AirQualityObserved' entities do not conform to their schema (first: urn:ngsi-ld:AirQualityObserved:bus-12)",
            "{store:?}"
        );
        assert_eq!(warned[0].occurrences, 1, "{store:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

#[test]
fn the_validation_report_lists_the_entity_once_with_the_member_none_of_its_records_holds() {
    for store in STORES {
        let folder = TempDir::new().unwrap();
        let report = folder.path().join("report.json");

        let (result, _) = run(
            store,
            folder.path(),
            BUS_SNAPSHOT,
            &placeless_mapping(),
            &format!(r#", validation: {{ mode: "warn", report: "{}" }}"#, report.display()),
        );
        result.unwrap();

        let written: Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
        assert_eq!(written["failedEntities"], 1, "{store:?}");
        let failures = written["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 1, "{store:?}");
        assert_eq!(failures[0]["entityId"], BUS, "{store:?}");
        let errors: Vec<&Value> = failures[0]["evaluation"]["details"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|unit| unit.get("errors"))
            .collect();
        assert_eq!(errors, [&json!({"required": "\"location\" is a required property"})], "{store:?}");
    }
}

#[test]
fn a_series_run_whose_records_hold_every_required_member_writes_no_report() {
    for store in STORES {
        let folder = TempDir::new().unwrap();
        let report = folder.path().join("report.json");

        let (result, _) = run(
            store,
            folder.path(),
            BUS_SNAPSHOT,
            &guide_mapping(),
            &format!(r#", validation: {{ report: "{}" }}"#, report.display()),
        );
        result.unwrap();

        assert!(!report.exists(), "{store:?}");
    }
}

/// The bus snapshot with one Observation, whose result is the text `high`.
fn high_snapshot() -> String {
    let mut snapshot = String::new();
    for line in BUS_SNAPSHOT.lines().filter(|line| !line.contains(r#""@iot.id":702"#)) {
        snapshot.push_str(&line.replace(r#""result":18.2"#, r#""result":"high""#));
        snapshot.push('\n');
    }
    snapshot
}

/// The track mapping with `pm10` read without a transformation, so its value is the text read.
fn untransformed_mapping() -> String {
    track_mapping("{{ historicalLocation.location }}", "")
}

#[test]
fn a_value_of_the_wrong_type_is_still_reported_for_the_record_that_carries_it() {
    let snapshot = high_snapshot();
    let mapping = untransformed_mapping();
    for store in STORES {
        let folder = TempDir::new().unwrap();

        let (result, outcome) = run(store, folder.path(), &snapshot, &mapping, "");

        // The record of the Observation lacks `location`, which is not decided on it, so its one
        // violation is its `pm10`.
        assert_eq!(
            failure(result),
            r#"Entity urn:ngsi-ld:AirQualityObserved:bus-12 of type 'AirQualityObserved' does not conform to its schema: 1 violation(s), first at '/pm10': "high" is not of type "number""#,
            "{store:?}"
        );
        assert_eq!(outcome.entities, Vec::<Value>::new(), "{store:?}");
    }
}

#[test]
fn a_record_s_wrong_value_is_one_warning_for_the_record_when_relaxed() {
    let snapshot = high_snapshot();
    let mapping = untransformed_mapping();
    for store in STORES {
        let folder = TempDir::new().unwrap();

        let (result, outcome) = run(store, folder.path(), &snapshot, &mapping, r#", validation: { mode: "warn" }"#);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), [BUS], "{store:?}");
        let bus = entity(&outcome, BUS);
        assert_eq!(values(&bus["pm10"]), [&json!("high")], "{store:?}");
        assert_eq!(instant_times(&bus["location"]).len(), 2, "{store:?}");
        let warned = outcome.coded(DiagnosticCode::Schema(SchemaCode::Nonconformant));
        assert_eq!(warned.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(warned[0].occurrences, 1, "{store:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

/// The bus snapshot with both Observations' results the text `high`.
fn both_high_snapshot() -> String {
    BUS_SNAPSHOT
        .replace(r#""result":18.2"#, r#""result":"high""#)
        .replace(r#""result":21.5"#, r#""result":"high""#)
}

#[test]
fn the_wrong_values_of_several_records_of_one_entity_are_one_warning_for_the_entity_when_relaxed() {
    let snapshot = both_high_snapshot();
    let mapping = untransformed_mapping();
    for store in STORES {
        let folder = TempDir::new().unwrap();

        let (result, outcome) = run(store, folder.path(), &snapshot, &mapping, r#", validation: { mode: "warn" }"#);
        result.unwrap();

        assert_eq!(entity_ids(&outcome), [BUS], "{store:?}");
        assert_eq!(values(&entity(&outcome, BUS)["pm10"]), [&json!("high"), &json!("high")], "{store:?}");
        let warned = outcome.coded(DiagnosticCode::Schema(SchemaCode::Nonconformant));
        assert_eq!(warned.len(), 1, "{store:?}: {:?}", outcome.diagnostics);
        assert_eq!(
            warned[0].headline, "1 'AirQualityObserved' entities do not conform to their schema (first: urn:ngsi-ld:AirQualityObserved:bus-12)",
            "{store:?}"
        );
        assert_eq!(warned[0].occurrences, 1, "{store:?}");
        assert_eq!(outcome.warnings, 1, "{store:?}");
    }
}

/// A latest snapshot of one station whose only Location is a text, so its record has no `location`.
const TEXT_LOCATED_SNAPSHOT: &str = r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"latest","collectedAt":"2026-11-16T09:00:00Z"}
{"datastream":{"@iot.id":11,"name":"station-1 pm10","unitOfMeasurement":{"symbol":"ug/m3"},"Thing":{"@iot.id":1,"name":"Stationsplein","Locations":[{"@iot.id":101,"encodingType":"text/plain","location":"Stationsplein 1, 3rd floor, room 3.12"}]},"ObservedProperty":{"@iot.id":1,"name":"pm10"},"Observations":[{"@iot.id":1101,"phenomenonTime":"2026-11-16T08:00:00Z","result":18.2}]}}
"#;

#[test]
fn a_latest_read_still_checks_a_required_member_on_each_record() {
    for store in STORES {
        let folder = TempDir::new().unwrap();
        let source = folder.path().join("station.jsonl");
        fs::write(&source, TEXT_LOCATED_SNAPSHOT).unwrap();

        let (result, outcome) = attempt_file_end_to_end(
            store,
            &source,
            &fixture("air-quality-observed.json5"),
            &RunSchemas::Folder(&fixture("schemas")),
            "",
        );

        assert_eq!(
            failure(result),
            r#"Entity urn:ngsi-ld:AirQualityObserved:sta-hexample-1 of type 'AirQualityObserved' does not conform to its schema: 1 violation(s), first at '': "location" is a required property"#,
            "{store:?}"
        );
        assert_eq!(outcome.entities, Vec::<Value>::new(), "{store:?}");
    }
}

use crate::end_to_end::{
    harness::{RunBinding, RunInput, RunOutcome, STORES, run_csv_end_to_end, run_end_to_end},
    sensorthings::SERIES_OUTPUT,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, transform_code::TransformCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The code an unreadable `observedAt` is published under, whatever transformation it declares.
const OBSERVED_AT_UNREADABLE: DiagnosticCode = DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable);

/// Pairs of records of one station whose `observedAt` texts sort in the opposite order to the
/// instants they name, or do not read at all.
const STATIONS: &str = "station,t,time
S1,20.0,2026-03-01T11:00:00Z
S1,25.0,2026-03-01T12:00:00+02:00
S2,7.0,999999999
S2,8.0,1000000000
S3,30.0,2026-03-01T11:00:00Z
S3,31.0,soon
S4,40.0,02/03/2026
S4,41.0,01/04/2026
";

const WEATHER_OBSERVED: &str = r#"{
    version: "v4",
    dataModel: "WeatherObserved",
    identity: { entityName: "{{ station }}" },
    attributes: {
        temperature: {
            source: "{{ t }}",
            transformation: "float",
            properties: {
                observedAt: { source: "{{ time }}", transformation: "datetime" },
            },
        },
    },
}"#;

/// Runs `csv` through `mapping` on `store`, with `output_extra` spliced into the output.
fn run(store: StoreKind, csv: &str, mapping: &'static str, entity_type: &str, output_extra: &str) -> RunOutcome {
    run_csv_end_to_end(store, csv, &RunBinding::Mapping(mapping), &[entity_type], output_extra)
}

/// The entity `id` of `outcome`.
fn entity<'a>(outcome: &'a RunOutcome, id: &str) -> &'a Value {
    outcome
        .entities
        .iter()
        .find(|entity| entity["id"] == id)
        .unwrap_or_else(|| panic!("no entity {id} was written"))
}

/// The value and `observedAt` of `attribute` of the entity `id`, `None` for an attribute it does not
/// hold and `null` for a qualifier the attribute does not carry.
fn observed(outcome: &RunOutcome, id: &str, attribute: &str) -> Option<(Value, Value)> {
    let attribute = entity(outcome, id).get(attribute)?;
    Some((attribute["value"].clone(), attribute.get("observedAt").cloned().unwrap_or(Value::Null)))
}

#[test]
fn each_station_keeps_the_record_naming_the_latest_instant_however_its_time_is_spelled_in_every_store() {
    for store in STORES {
        let outcome = run(store, STATIONS, WEATHER_OBSERVED, "WeatherObserved", "");

        let kept: Vec<Option<(Value, Value)>> = ["S1", "S2", "S3", "S4"]
            .iter()
            .map(|station| observed(&outcome, &format!("urn:ngsi-ld:WeatherObserved:{station}"), "temperature"))
            .collect();
        assert_eq!(
            kept,
            [
                Some((json!(20.0), json!("2026-03-01T11:00:00Z"))),
                Some((json!(8.0), json!("2001-09-09T01:46:40Z"))),
                Some((json!(30.0), json!("2026-03-01T11:00:00Z"))),
                Some((json!(41.0), json!("2026-04-01T00:00:00Z"))),
            ],
            "{store:?}"
        );
    }
}

#[test]
fn the_unreadable_time_of_a_discarded_record_is_reported_once_in_every_store() {
    for store in STORES {
        let outcome = run(store, STATIONS, WEATHER_OBSERVED, "WeatherObserved", "");

        let reported = outcome.coded(OBSERVED_AT_UNREADABLE);
        assert_eq!(reported.len(), 1, "{store:?}");
        assert_eq!(
            reported[0].headline, "Attribute `temperature` emitted without `observedAt`: `soon` is not a date-time in any spelling Cassiopeia reads",
            "{store:?}"
        );
        assert_eq!(reported[0].occurrences, 1, "{store:?}");
    }
}

#[test]
fn a_series_keeps_every_observation_ranking_would_discard_in_every_store() {
    for store in STORES {
        let outcome = run(store, STATIONS, WEATHER_OBSERVED, "WeatherObserved", SERIES_OUTPUT);

        // The reading whose time is `soon` carries no `observedAt`, so the series holds no
        // observation of it, as it held none before records were ranked by instant.
        let series: Vec<Vec<(Value, Value)>> = ["S1", "S2", "S3", "S4"]
            .iter()
            .map(|station| {
                entity(&outcome, &format!("urn:ngsi-ld:WeatherObserved:{station}"))["temperature"]
                    .as_array()
                    .map_or_else(Vec::new, |instances| {
                        instances
                            .iter()
                            .map(|instance| (instance["value"].clone(), instance["observedAt"].clone()))
                            .collect()
                    })
            })
            .collect();
        assert_eq!(
            series,
            [
                vec![(json!(25.0), json!("2026-03-01T10:00:00Z")), (json!(20.0), json!("2026-03-01T11:00:00Z"))],
                vec![(json!(7.0), json!("2001-09-09T01:46:39Z")), (json!(8.0), json!("2001-09-09T01:46:40Z"))],
                vec![(json!(30.0), json!("2026-03-01T11:00:00Z"))],
                vec![(json!(40.0), json!("2026-03-02T00:00:00Z")), (json!(41.0), json!("2026-04-01T00:00:00Z"))],
            ],
            "{store:?}"
        );
    }
}

/// Two attributes, each observed at a time of its own, beside one observed at none.
const READING_AND_STATUS: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: {
        reading: { source: "{{ reading }}", transformation: "float", properties: { observedAt: { source: "{{ read_time }}" } } },
        status: { source: "{{ status }}", properties: { observedAt: { source: "{{ status_time }}" } } },
        name: { source: "{{ name }}" },
    },
}"#;

/// The first record observed `reading` last and the last record observed `status` last.
const SENSOR_RECORDS: &str = "code,reading,read_time,status,status_time,name
S1,22.5,2026-03-01T11:00:00Z,warm,2026-03-01T09:00:00Z,First
S1,21.5,2026-03-01T10:00:00Z,ok,2026-03-01T12:00:00Z,Second
";

/// The first record holds the latest observation over every attribute, though not of `status`.
const NEWEST_FIRST: &str = "code,reading,read_time,status,status_time,name
S1,22.5,2026-03-01T13:00:00Z,warm,2026-03-01T09:00:00Z,First
S1,21.5,2026-03-01T10:00:00Z,ok,2026-03-01T12:00:00Z,Second
";

/// Two instances of one forecast, each observed at a time of its own.
const FORECASTS: &str = r#"{
    version: "v4",
    dataModel: "WeatherForecast",
    identity: { entityName: "{{ code }}" },
    attributes: {
        temperature: {
            transformation: "float",
            instances: [
                { source: "{{ ecmwf }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:ecmwf" }, observedAt: { source: "{{ ecmwf_time }}" } } },
                { source: "{{ gfs }}", properties: { datasetId: { source: "urn:ngsi-ld:Dataset:gfs" }, observedAt: { source: "{{ gfs_time }}" } } },
            ],
        },
    },
}"#;

/// A reading whose only `observedAt` qualifies its accuracy.
const ACCURACY_OBSERVED: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: {
        reading: {
            source: "{{ reading }}",
            transformation: "float",
            properties: { accuracy: { source: "{{ accuracy }}", properties: { observedAt: { source: "{{ time }}" } } } },
        },
    },
}"#;

/// A station that declares no `observedAt`.
const STATION: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: { name: { source: "{{ name }}" }, capacity: { source: "{{ capacity }}" } },
}"#;

/// `csv` with its records after the header in reverse order.
fn reversed(csv: &str) -> String {
    let mut lines: Vec<&str> = csv.lines().collect();
    lines[1..].reverse();
    lines.join("\n") + "\n"
}

#[test]
fn each_attribute_comes_from_the_record_where_it_was_observed_last_in_every_store() {
    for store in STORES {
        let outcome = run(store, SENSOR_RECORDS, READING_AND_STATUS, "Sensor", "");

        let id = "urn:ngsi-ld:Sensor:S1";
        assert_eq!(
            observed(&outcome, id, "reading"),
            Some((json!(22.5), json!("2026-03-01T11:00:00Z"))),
            "{store:?}"
        );
        assert_eq!(
            observed(&outcome, id, "status"),
            Some((json!("ok"), json!("2026-03-01T12:00:00Z"))),
            "{store:?}"
        );
    }
}

#[test]
fn an_attribute_without_observed_at_comes_from_the_record_observed_last_whatever_the_order_in_every_store() {
    for store in STORES {
        for csv in [NEWEST_FIRST.to_owned(), reversed(NEWEST_FIRST)] {
            let outcome = run(store, &csv, READING_AND_STATUS, "Sensor", "");

            let id = "urn:ngsi-ld:Sensor:S1";
            assert_eq!(observed(&outcome, id, "name"), Some((json!("First"), Value::Null)), "{store:?} {csv}");
            assert_eq!(
                observed(&outcome, id, "status"),
                Some((json!("ok"), json!("2026-03-01T12:00:00Z"))),
                "{store:?} {csv}"
            );
        }
    }
}

#[test]
fn each_instance_comes_from_the_record_where_it_was_observed_last_in_every_store() {
    let csv = "code,ecmwf,ecmwf_time,gfs,gfs_time
F1,21.5,2026-03-01T11:00:00Z,22.0,2026-03-01T09:00:00Z
F1,23.5,2026-03-01T10:00:00Z,24.0,2026-03-01T12:00:00Z
";
    for store in STORES {
        let outcome = run(store, csv, FORECASTS, "WeatherForecast", "");

        let instances: Vec<(Value, Value, Value)> = entity(&outcome, "urn:ngsi-ld:WeatherForecast:F1")["temperature"]
            .as_array()
            .unwrap()
            .iter()
            .map(|instance| (instance["value"].clone(), instance["observedAt"].clone(), instance["datasetId"].clone()))
            .collect();
        assert_eq!(
            instances,
            [
                (json!(21.5), json!("2026-03-01T11:00:00Z"), json!("urn:ngsi-ld:Dataset:ecmwf")),
                (json!(24.0), json!("2026-03-01T12:00:00Z"), json!("urn:ngsi-ld:Dataset:gfs")),
            ],
            "{store:?}"
        );
    }
}

/// Two attributes sharing one `observedAt` template.
const TEMPERATURE_AND_STATUS: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: {
        temperature: { source: "{{ t }}", transformation: "float", properties: { observedAt: { source: "{{ time }}", transformation: "datetime" } } },
        status: { source: "{{ status }}", properties: { observedAt: { source: "{{ time }}", transformation: "datetime" } } },
    },
}"#;

/// Of each sensor's records, the one observed last holds no temperature: `S1` lists it last and
/// `S2` first.
const NEWEST_WITHOUT_TEMPERATURE: &str = "code,t,status,time
S1,20,ok,2026-03-01T10:00:00Z
S1,,warm,2026-03-01T11:00:00Z
S2,,warm,2026-03-01T11:00:00Z
S2,20,ok,2026-03-01T10:00:00Z
";

#[test]
fn a_shared_observed_at_keeps_the_record_observed_last_whole_leaving_out_a_value_it_lacks_in_every_store() {
    for store in STORES {
        let outcome = run(store, NEWEST_WITHOUT_TEMPERATURE, TEMPERATURE_AND_STATUS, "Sensor", "");

        for code in ["S1", "S2"] {
            let id = format!("urn:ngsi-ld:Sensor:{code}");
            assert_eq!(observed(&outcome, &id, "temperature"), None, "{store:?} {code}");
            assert_eq!(
                observed(&outcome, &id, "status"),
                Some((json!("warm"), json!("2026-03-01T11:00:00Z"))),
                "{store:?} {code}"
            );
        }
    }
}

#[test]
fn an_observed_at_on_a_sub_attribute_alone_ranks_the_records_in_every_store() {
    let csv = "code,reading,accuracy,time
S1,22.5,0.5,2026-03-01T11:00:00Z
S1,21.5,0.4,2026-03-01T10:00:00Z
";
    for store in STORES {
        let outcome = run(store, csv, ACCURACY_OBSERVED, "Sensor", "");

        assert_eq!(
            observed(&outcome, "urn:ngsi-ld:Sensor:S1", "reading").map(|(value, _)| value),
            Some(json!(22.5)),
            "{store:?}"
        );
    }
}

#[test]
fn records_of_a_mapping_without_observed_at_merge_into_one_entity_in_every_store() {
    for store in STORES {
        let outcome = run_end_to_end(
            store,
            &[RunInput {
                binding: RunBinding::Mapping(STATION),
                records: json!([{ "code": "S1", "name": "Main" }, { "code": "S1", "capacity": "40" }]),
            }],
            &["Station"],
            "",
        );

        let id = "urn:ngsi-ld:Station:S1";
        assert_eq!(observed(&outcome, id, "name").map(|(value, _)| value), Some(json!("Main")), "{store:?}");
        assert_eq!(observed(&outcome, id, "capacity").map(|(value, _)| value), Some(json!("40")), "{store:?}");
    }
}

#[test]
fn a_series_over_three_records_writes_three_observations_of_each_attribute_in_every_store() {
    let csv = format!("{SENSOR_RECORDS}S1,20.5,2026-03-01T08:00:00Z,cold,2026-03-01T08:00:00Z,Third\n");
    for store in STORES {
        let outcome = run(store, &csv, READING_AND_STATUS, "Sensor", SERIES_OUTPUT);

        let sensor = entity(&outcome, "urn:ngsi-ld:Sensor:S1");
        let counts: Vec<usize> = ["reading", "status"]
            .iter()
            .map(|attribute| sensor[attribute].as_array().map_or(1, Vec::len))
            .collect();
        assert_eq!(counts, [3, 3], "{store:?}");
    }
}

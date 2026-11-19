//! A series run folds every record of an entity into the one temporal entity it writes. An attribute
//! without `observedAt` is kept from the entity's first record in the source, whichever order the
//! records reach the fold in and whichever of them is the oldest, and the file lists the entities in
//! the order of their first records.

use crate::end_to_end::{
    harness::{RunBinding, listed_ids, run_csv_end_to_end},
    repeated_runs::assert_every_run_and_store_writes_the_same,
    sensorthings::{SERIES_OUTPUT, entity},
};
use pretty_assertions::assert_eq;

/// A sensor with a `temperature` it observes and a `status` it does not.
const SENSOR: &str = r#"{
    version: "v4",
    dataModel: "Sensor",
    identity: { entityName: "{{ code }}" },
    attributes: {
        status: { source: "{{ status }}" },
        temperature: { source: "{{ t }}", transformation: "integer", properties: { observedAt: { source: "{{ time }}" } } },
    },
}"#;

/// Three records of each of two sensors, interleaved and newest first, each with a status of its own.
const READINGS: &str = "code,status,t,time\n\
                        S2,warm,21,2026-03-01T12:00:00Z\n\
                        S1,ok,20,2026-03-01T12:00:00Z\n\
                        S2,cold,19,2026-03-01T11:00:00Z\n\
                        S1,warn,18,2026-03-01T11:00:00Z\n\
                        S2,off,17,2026-03-01T10:00:00Z\n\
                        S1,idle,16,2026-03-01T10:00:00Z\n";

#[test]
fn an_attribute_without_observed_at_the_records_disagree_on_is_kept_from_the_first_record_in_the_source_on_every_run_and_store() {
    assert_every_run_and_store_writes_the_same(
        |store| run_csv_end_to_end(store, READINGS, &RunBinding::Mapping(SENSOR), &["Sensor"], SERIES_OUTPUT),
        |outcome| {
            assert_eq!(listed_ids(&outcome.entities), ["urn:ngsi-ld:Sensor:S2", "urn:ngsi-ld:Sensor:S1"]);
            // Each sensor's first record in the source is its newest, so the status kept is neither
            // the oldest record's nor the last record's.
            assert_eq!(entity(outcome, "urn:ngsi-ld:Sensor:S2")["status"]["value"], "warm");
            assert_eq!(entity(outcome, "urn:ngsi-ld:Sensor:S1")["status"]["value"], "ok");
        },
    );
}

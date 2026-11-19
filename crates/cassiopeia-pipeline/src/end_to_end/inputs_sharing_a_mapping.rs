//! Inputs of one manifest that share a mapping without `observedAt` merge the records they give one
//! entity on every run and entity store as if every record were merged in the order of the source: a
//! field the inputs disagree on keeps the value of the first input in the manifest that holds it, and
//! within that input of its first record that holds it, while a field only a later input holds is
//! taken from that input. Inputs that read the mapping with different `vars` render their records
//! through it as two mappings, and an attribute they disagree on is still taken from the first input
//! in the manifest.

use crate::end_to_end::{
    harness::{RunOutcome, attempt_inputs_end_to_end},
    repeated_runs::{assert_every_run_and_store_writes_the_same, only_entity},
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, resolver_code::ResolverCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::fs;
use temp_dir::TempDir;

/// The code a record-merge warning is published under.
const RECORDS_MERGED: DiagnosticCode = DiagnosticCode::Resolver(ResolverCode::RecordsMerged);

/// A station mapping without `observedAt`, which every input of a run shares.
const STATIONS: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ code }}" },
    attributes: {
        name: { source: "{{ name }}" },
        capacity: { source: "{{ capacity }}", transformation: "integer" },
        zone: { source: "{{ zone }}" },
    },
}"#;

/// One record of the station `S1` with a capacity of 40.
const NORTH: &str = "code,name,capacity\nS1,Main,40\n";

/// One record of the station `S1` with a capacity of 42 and a zone.
const SOUTH: &str = "code,name,capacity,zone\nS1,Main,42,B\n";

/// The inputs of one run, each reading one of `sources`, in order, and every one of them mapped by
/// the one mapping file of [`STATIONS`], all kept in a folder of their own.
struct SharedMapping {
    /// The folder holding the mapping file and every input's records.
    _folder: TempDir,
    /// The inputs, as a manifest declares them, in the order of `sources`.
    declared: Vec<String>,
}

impl SharedMapping {
    fn new(sources: &[&str]) -> SharedMapping {
        SharedMapping::declaring(sources.iter().map(|records| (*records, "")))
    }

    /// The inputs of one run, each reading the records of one of `sources`, in order, with the
    /// members, such as `vars`, that its pair splices into the input's declaration.
    fn declaring<'s>(sources: impl IntoIterator<Item = (&'s str, &'s str)>) -> SharedMapping {
        let folder = TempDir::new().unwrap();
        let mapping = folder.path().join("stations.json5");
        fs::write(&mapping, STATIONS).unwrap();
        let declared = sources
            .into_iter()
            .enumerate()
            .map(|(index, (records, members))| {
                let data = folder.path().join(format!("stations-{index}.csv"));
                fs::write(&data, records).unwrap();
                format!(r#"{{ source: "{}", mapping: "{}"{members} }}"#, data.display(), mapping.display())
            })
            .collect();
        SharedMapping { _folder: folder, declared }
    }

    /// Runs the inputs on `store` and returns what the run wrote and reported.
    fn run(&self, store: StoreKind) -> RunOutcome {
        let (result, outcome) = attempt_inputs_end_to_end(store, &self.declared, &["Station"]);
        result.unwrap();
        outcome
    }
}

/// The value of the station's `attribute`.
fn value_of<'a>(station: &'a Value, attribute: &str) -> &'a Value {
    &station[attribute]["value"]
}

/// Asserts that `outcome` reported one merge warning, standing for one entity whose `records` records
/// disagreed on `capacity`.
fn assert_reports_the_merge(outcome: &RunOutcome, records: u64) {
    let merges: Vec<(&str, u64)> = outcome
        .coded(RECORDS_MERGED)
        .iter()
        .map(|merge| (merge.headline.as_str(), merge.occurrences))
        .collect();
    let headline = format!(
        "{records} records resolved to `urn:ngsi-ld:Station:S1` and disagreed on `capacity`; one value of each disagreeing field was kept and the others were discarded"
    );
    assert_eq!(merges, [(headline.as_str(), 1)]);
}

#[test]
fn a_field_two_inputs_disagree_on_keeps_the_first_input_s_value_on_every_run_and_store() {
    let inputs = SharedMapping::new(&[NORTH, SOUTH]);
    assert_every_run_and_store_writes_the_same(
        |store| inputs.run(store),
        |outcome| {
            let station = only_entity(outcome);
            assert_eq!(value_of(station, "name"), &json!("Main"));
            assert_eq!(value_of(station, "capacity"), &json!(40));
            assert_eq!(value_of(station, "zone"), &json!("B"));
            assert_reports_the_merge(outcome, 2);
        },
    );
}

#[test]
fn the_two_inputs_swapped_in_the_manifest_keep_the_other_input_s_value_on_every_run_and_store() {
    let inputs = SharedMapping::new(&[SOUTH, NORTH]);
    assert_every_run_and_store_writes_the_same(
        |store| inputs.run(store),
        |outcome| {
            let station = only_entity(outcome);
            assert_eq!(value_of(station, "capacity"), &json!(42));
            assert_eq!(value_of(station, "zone"), &json!("B"));
            assert_reports_the_merge(outcome, 2);
        },
    );
}

#[test]
fn three_inputs_keep_the_earliest_input_s_value_and_take_a_field_only_a_later_one_holds_on_every_run_and_store() {
    let inputs = SharedMapping::new(&[
        "code,capacity\nS1,40\nS1,41\n",
        "code,name,capacity\nS1,Main,42\n",
        "code,name,capacity,zone\nS1,Renamed,44,C\n",
    ]);
    assert_every_run_and_store_writes_the_same(
        |store| inputs.run(store),
        |outcome| {
            let station = only_entity(outcome);
            assert_eq!(value_of(station, "capacity"), &json!(40));
            assert_eq!(value_of(station, "name"), &json!("Main"));
            assert_eq!(value_of(station, "zone"), &json!("C"));
            assert_reports_the_merge(outcome, 4);
        },
    );
}

#[test]
fn inputs_that_only_add_fields_to_one_another_report_no_merge_on_any_store() {
    let inputs = SharedMapping::new(&["code,name,capacity\nS1,Main,40\n", "code,zone\nS1,B\n"]);
    assert_every_run_and_store_writes_the_same(
        |store| inputs.run(store),
        |outcome| {
            let station = only_entity(outcome);
            assert_eq!(value_of(station, "capacity"), &json!(40));
            assert_eq!(value_of(station, "zone"), &json!("B"));
            assert!(outcome.coded(RECORDS_MERGED).is_empty(), "{:?}", outcome.diagnostics);
        },
    );
}

#[test]
fn inputs_reading_the_mapping_with_different_vars_keep_the_first_input_s_value_on_every_run_and_store() {
    let inputs = SharedMapping::declaring([(NORTH, r#", vars: { network: "north" }"#), (SOUTH, r#", vars: { network: "south" }"#)]);
    assert_every_run_and_store_writes_the_same(
        |store| inputs.run(store),
        |outcome| {
            let station = only_entity(outcome);
            assert_eq!(value_of(station, "name"), &json!("Main"));
            assert_eq!(value_of(station, "capacity"), &json!(40));
            assert_eq!(value_of(station, "zone"), &json!("B"));
        },
    );
}

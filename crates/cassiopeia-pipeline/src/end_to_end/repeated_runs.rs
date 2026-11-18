//! Running one end-to-end input many times on every entity store, to show that what a run writes
//! changes neither from one run to the next nor from one store to the other.

use crate::end_to_end::harness::{RunOutcome, STORES};
use cassiopeia_common::store_kind::StoreKind;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

/// How many times each store runs the same input, so an order that changed between runs would show.
const RUNS: usize = 20;

/// The one entity `outcome` wrote.
pub(crate) fn only_entity(outcome: &RunOutcome) -> &Value {
    let [entity] = outcome.entities.as_slice() else {
        panic!("one entity must be written: {:?}", outcome.entities);
    };
    entity
}

/// Runs `run` [`RUNS`] times on each store, checks what every run wrote and reported with `check`,
/// and asserts that every run wrote the same files, byte for byte.
pub(crate) fn assert_every_run_and_store_writes_the_same(run: impl Fn(StoreKind) -> RunOutcome, check: impl Fn(&RunOutcome)) {
    let mut written: Vec<BTreeMap<PathBuf, String>> = Vec::new();
    for store in STORES {
        for _ in 0..RUNS {
            let outcome = run(store);
            check(&outcome);
            written.push(outcome.files);
        }
    }
    assert_eq!(written.len(), STORES.len() * RUNS);
    assert!(written.windows(2).all(|pair| pair[0] == pair[1]));
}

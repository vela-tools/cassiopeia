//! The concurrent write path of [`FragmentResolver`]: every fragment of a batch is prepared and
//! written on its own, across threads, which is what a store taking concurrent writes needs.

use crate::{
    error::Result,
    fragment_resolver::{FragmentResolver, ResolveBatchTiming},
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
use rayon::prelude::*;
use std::time::{Duration, Instant};

impl FragmentResolver {
    /// Resolves a batch against stores that accept concurrent per-fragment writes.
    ///
    /// The phases are the same as the coalesced path: prepare, write relationships, write the
    /// fragment. Each fragment carries its own here, so the writes of one batch reach the store in
    /// whatever order the threads finish them, all of them before any write of the next batch. A
    /// store whose outcome depends on the order of an entity's records restores it from the
    /// positions and the batch the writes carry. The timing spans the whole fan-out rather than one
    /// shared flush; that is what lets a concurrent store's write time be measured at all, since no
    /// single flush exists to time.
    pub(crate) fn resolve_batch_concurrent(&self, fragments: Vec<Mapped<Fragment>>) -> (Vec<Result<()>>, ResolveBatchTiming) {
        let started = Instant::now();
        let batch = self.write_batches.next();
        let interned = self.intern_distinct(fragments.iter().map(|fragment| (fragment.mapping(), fragment.inner().variables())));
        let resolve = |fragment: Mapped<Fragment>| {
            let mapping_id = self.interned_id(&interned, fragment.mapping(), fragment.inner().variables());
            self.write_prepared(self.prepare(fragment, mapping_id), batch)
        };
        let results = match self.parallelism {
            Parallelism::Parallel => fragments.into_par_iter().map(resolve).collect(),
            Parallelism::Sequential => fragments.into_iter().map(resolve).collect(),
        };
        // The per-fragment phases interleave on this path, so the elapsed span is attributed to the
        // store write as a whole rather than split between preparation and the two stores.
        (
            results,
            ResolveBatchTiming {
                preparation: Duration::ZERO,
                relationship_write: Duration::ZERO,
                entity_write: started.elapsed(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_source::EntitySource,
        entity_store::{dashmap_latest_store::DashMapLatestEntityStore, redb_latest_store::RedbLatestEntityStore, store::EntityStore},
        fragment_resolver::FragmentResolver,
        fragment_sink::FragmentSink,
        relationship_store::dashmap_store::DashMapRelationshipStore,
    };
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, source_position::SourcePosition};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    const STATION: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: { name: { source: "{{ name }}" }, capacity: { source: "{{ capacity }}" } },
    }"#;

    const OBSERVED_STATION: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: {
            temperature: { source: "{{ t }}", properties: { observedAt: { source: "{{ time }}" } } },
        },
    }"#;

    /// How many times each store is run over the same batch, so an order that changed between runs
    /// would show.
    const RUNS: usize = 20;

    fn current_state_stores() -> [Box<dyn EntityStore>; 2] {
        [Box::new(DashMapLatestEntityStore::new()), Box::new(RedbLatestEntityStore::new().unwrap())]
    }

    /// Resolves `records`, in order, as one batch through `document` over `store`, checking that
    /// every one of them was written.
    fn resolved(store: Box<dyn EntityStore>, document: &str, records: &[Value]) -> FragmentResolver {
        let mut runner = TemplateRunner::new();
        let mapping = Arc::new(Mapping::from_json5(document, Path::new("station.json5"), &mut runner).unwrap());
        let resolver = FragmentResolver::new(store, Box::new(DashMapRelationshipStore::new()), runner.resolver());
        let first = SourcePosition::first_of(0);
        let batch = records
            .iter()
            .enumerate()
            .map(|(record, data)| {
                let target: Urn = format!("urn:ngsi-ld:Station:{}", data["code"].as_str().unwrap()).parse().unwrap();
                Mapped::new(
                    Fragment::builder()
                        .source_data(data.clone())
                        .target_urn(target)
                        .position(first.advanced(record))
                        .build(),
                    Arc::clone(&mapping),
                )
            })
            .collect();
        let results = resolver.resolve_batch(batch);
        assert_eq!(results.len(), records.len());
        assert!(results.iter().all(Result::is_ok));
        resolver
    }

    /// The record `resolved` keeps for `id`.
    fn kept_record(store: Box<dyn EntityStore>, document: &str, records: &[Value], id: &Urn) -> Value {
        resolved(store, document, records).assemble(id).unwrap().units[0].fragments()[0]
            .record()
            .clone()
    }

    /// A batch of 400 records made by `record`, which spreads the records of `S1` among those of
    /// another station.
    fn interleaved(record: impl Fn(usize) -> Value) -> Vec<Value> {
        (0..400).map(record).collect()
    }

    #[test]
    fn a_field_the_records_of_one_batch_disagree_on_keeps_the_first_record_s_value_in_every_run_and_store() {
        let s1: Urn = "urn:ngsi-ld:Station:S1".parse().unwrap();
        let records = interleaved(|record| {
            let code = if record % 3 == 0 { "S1" } else { "S2" };
            json!({"code": code, "name": format!("Name {record}"), "capacity": record})
        });
        for _ in 0..RUNS {
            for store in current_state_stores() {
                let kept = kept_record(store, STATION, &records, &s1);
                assert_eq!((&kept["name"], &kept["capacity"]), (&json!("Name 0"), &json!(0)));
            }
        }
    }

    #[test]
    fn records_naming_the_same_observed_at_keep_the_one_last_in_the_source_in_every_run_and_store() {
        let s1: Urn = "urn:ngsi-ld:Station:S1".parse().unwrap();
        let records = interleaved(|record| {
            let code = if record % 3 == 0 { "S1" } else { "S2" };
            json!({"code": code, "t": record, "time": "2026-03-01T11:00:00Z"})
        });
        for _ in 0..RUNS {
            for store in current_state_stores() {
                assert_eq!(kept_record(store, OBSERVED_STATION, &records, &s1)["t"], json!(399));
            }
        }
    }

    #[test]
    fn a_batch_spread_over_many_lanes_writes_every_fragment_once() {
        let records: Vec<Value> = (0..1000)
            .map(|record| json!({"code": format!("S{record}"), "name": "x", "capacity": record}))
            .collect();

        let resolver = resolved(Box::new(DashMapLatestEntityStore::new()), STATION, &records);

        assert_eq!(resolver.get_unique_entity_count().unwrap(), 1000);
    }
}

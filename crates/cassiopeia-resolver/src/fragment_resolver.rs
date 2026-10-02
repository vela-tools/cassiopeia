use crate::{
    entity_store::store::{EntityStore, FragmentWrite},
    error::{ResolverError, Result},
    fragment_sink::FragmentSink,
    mapping_id::MappingId,
    mapping_registry::MappingRegistry,
    relationship_store::store::RelationshipStore,
    store_batch_failure::StoreBatchFailure,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::RandomState;
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{
    fragment::Fragment,
    mapped::Mapped,
    parent_context::{ParentContext, ParentContextType},
};
use cassiopeia_mapping::{mapping::Mapping, template::resolver::TemplateResolver};
use dashmap::DashMap;
use rayon::prelude::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use urn_rs::Urn;

/// Standard implementation of the [`FragmentSink`] and
/// [`EntitySource`](crate::entity_source::EntitySource) traits.
///
/// Uses pluggable entity and relationship stores to handle fragment storage and relationship tracking
/// while fragments are stored, then assembles complete entities on request. This module owns the
/// write path; the read-back path lives in [`entity_assembly`](crate::entity_assembly).
///
/// # Mapping tracking
///
/// Instead of a per-URN map of `Arc<Mapping>`, the resolver keeps a compact [`MappingRegistry`] of
/// O(M) entries and stores the resulting [`MappingId`] alongside each fragment. This handles the
/// multi-input case where two inputs produce fragments of the same NGSI-LD entity type under
/// different mappings: each mapping gets its own id, read back during assembly.
///
/// # Temporality
///
/// Temporality is an attribute concern (ETSI GS CIM 009 v1.9.1 clause 4.5.5): the store keys by the
/// base entity id and every mapping and every observation of one id merges into one assembly. A
/// temporal mapping's record-level `observedAt` is read once per record and passed to the store, which
/// either ranks records by it (current-state) or keeps every observation (series).
#[derive(Clone)]
pub struct FragmentResolver {
    pub(crate) entity_store: Box<dyn EntityStore>,
    pub(crate) relationship_store: Box<dyn RelationshipStore>,
    pub(crate) mapping_registry: Arc<MappingRegistry>,
    pub(crate) resolver: TemplateResolver,
    pub(crate) parallelism: Parallelism,
    write_strategy: StoreWriteStrategy,
    /// Deduplicates a temporal mapping's identical `(parent, property, child)` edges so a static
    /// relationship declared on every observation does not accumulate one duplicate per observation
    /// under the base-id key.
    temporal_edge_dedup: Arc<DashMap<(Urn, String, Urn), (), RandomState>>,
}

impl FragmentResolver {
    /// Creates a resolver over the given stores and the shared template resolver.
    ///
    /// The resolver is used to read each fragment's `observedAt` value; it is the same engine the
    /// expander compiled its templates against.
    #[must_use]
    pub fn new(entity_store: Box<dyn EntityStore>, relationship_store: Box<dyn RelationshipStore>, resolver: TemplateResolver) -> FragmentResolver {
        let write_strategy = entity_store.write_strategy().combine(relationship_store.write_strategy());

        FragmentResolver {
            entity_store,
            relationship_store,
            mapping_registry: Arc::new(MappingRegistry::new()),
            resolver,
            parallelism: Parallelism::Parallel,
            write_strategy,
            temporal_edge_dedup: Arc::new(DashMap::with_hasher(RandomState::new())),
        }
    }

    /// Sets whether batch preparation runs across threads.
    #[must_use]
    pub const fn with_parallelism(mut self, parallelism: Parallelism) -> FragmentResolver {
        self.parallelism = parallelism;
        self
    }

    /// Builds the parent-child edges a fragment records, keyed on base URNs.
    ///
    /// A temporal mapping re-declares its static relationships on every observation, so an identical
    /// `(parent, property, child)` edge from a temporal mapping is emitted once and later duplicates
    /// are dropped; a non-temporal mapping keeps every edge, preserving the retain-duplicates contract.
    pub(crate) fn build_edges(&self, base_urn: &Urn, parent_contexts: Option<&Vec<ParentContext>>, temporal: bool) -> Vec<(Urn, String, Urn)> {
        let Some(parent_contexts) = parent_contexts else {
            return Vec::new();
        };

        let mut edges = Vec::with_capacity(parent_contexts.len());
        for parent_context in parent_contexts {
            // The store key is the relationship key's text form: a bare name for a top-level
            // relationship, dotted segments for a nested one, and `name#index` for one instance of a
            // multi-attribute relationship, so the store groups each instance's objects apart.
            let property = parent_context.key().to_string();
            let (parent_urn, child_urn) = match parent_context.urn() {
                ParentContextType::Parent(parent) => (parent.clone(), base_urn.clone()),
                ParentContextType::Child(child) => (base_urn.clone(), child.clone()),
            };

            if temporal
                && self
                    .temporal_edge_dedup
                    .insert((parent_urn.clone(), property.clone(), child_urn.clone()), ())
                    .is_some()
            {
                continue;
            }
            edges.push((parent_urn, property, child_urn));
        }
        edges
    }
}

/// Timing of the distinct phases in one resolver batch.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveBatchTiming {
    /// Time spent extracting timestamps, preparing relationships, and interning mappings.
    pub preparation: Duration,
    /// Time spent writing relationship state to the relationship store.
    pub relationship_write: Duration,
    /// Time spent writing fragments to the entity store.
    pub entity_write: Duration,
}

impl ResolveBatchTiming {
    /// Returns the time spent in store operations, excluding resolver preparation.
    #[must_use]
    pub const fn store_write(self) -> Duration {
        self.relationship_write.saturating_add(self.entity_write)
    }
}

impl FragmentResolver {
    /// Resolves a batch and returns timing for preparation and the two store writes.
    ///
    /// The store timings intentionally describe the complete store operation. They do not claim to be
    /// commit-only timings: a backend may spend part of that interval opening a transaction, merging
    /// values, encoding data, or waiting for a writer before its commit call.
    ///
    /// The coalesced path splits each fragment into the side-effects to commit
    /// ([`prepare_fragments`](FragmentResolver::prepare_fragments), in
    /// [`coalesced_batch`](crate::coalesced_batch)), then flushes the relationship and
    /// entity stores once each. This turns an O(N) disk-backed run into O(batches) round-trips without
    /// changing single-fragment semantics.
    #[must_use]
    pub fn resolve_batch_timed(&self, fragments: Vec<Mapped<Fragment>>) -> (Vec<Result<()>>, ResolveBatchTiming) {
        if self.write_strategy == StoreWriteStrategy::Concurrent {
            return self.resolve_batch_concurrent(fragments);
        }

        let (successes, preparation) = self.prepare_fragments(fragments);
        let count = successes.len();
        let (rel_failure, relationship_write) = self.write_relationship_edges(&successes);
        let (ent_failure, entity_write) = self.write_entity_fragments(successes);

        // A store batch is all-or-nothing, so fan the failure across every fragment that contributed.
        // Sharing it keeps every fragment's result pointing at the same typed failure rather than at
        // a copy of its rendered text.
        let final_results = match StoreBatchFailure::of(rel_failure, ent_failure) {
            Some(failure) => {
                let failure = Arc::new(failure);
                (0..count).map(|_| Err(ResolverError::BatchFailure { source: Arc::clone(&failure) })).collect()
            }
            None => (0..count).map(|_| Ok(())).collect(),
        };

        (
            final_results,
            ResolveBatchTiming {
                preparation,
                relationship_write,
                entity_write,
            },
        )
    }

    /// Resolves a batch against stores that accept concurrent per-fragment writes.
    ///
    /// The phases are the same as the coalesced path: prepare, write relationships, write the
    /// fragment. Each fragment carries its own here, though, so the timing spans the whole fan-out
    /// rather than one shared flush; that is what lets a concurrent store's write time be measured
    /// at all, since no single flush exists to time.
    fn resolve_batch_concurrent(&self, fragments: Vec<Mapped<Fragment>>) -> (Vec<Result<()>>, ResolveBatchTiming) {
        let started = Instant::now();
        let interned = self.intern_distinct(fragments.iter().map(Mapped::mapping));
        let resolve = |fragment: Mapped<Fragment>| {
            let mapping_id = self.interned_id(&interned, fragment.mapping());
            self.resolve_with_mapping_id(fragment, mapping_id)
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

    /// Interns every distinct mapping in `mappings`, keyed by `Arc` pointer identity, and returns
    /// the resulting table.
    ///
    /// A run wires one `Arc<Mapping>` per input and clones it onto every fragment it produces, so
    /// the table holds a single entry in practice and a linear scan beats any map. Interning once
    /// per batch instead of once per fragment is what keeps every worker off the registry's one
    /// matching shard.
    pub(crate) fn intern_distinct<'a>(&self, mappings: impl Iterator<Item = &'a Arc<Mapping>>) -> Vec<(usize, MappingId)> {
        let mut interned: Vec<(usize, MappingId)> = Vec::new();
        for mapping in mappings {
            let ptr = Arc::as_ptr(mapping) as usize;
            if interned.iter().any(|(known, _)| *known == ptr) {
                continue;
            }
            interned.push((ptr, self.mapping_registry.intern(mapping)));
        }
        interned
    }

    /// Reads a mapping's id out of a table built by
    /// [`intern_distinct`](FragmentResolver::intern_distinct), interning through the registry for a
    /// pointer the table does not cover.
    pub(crate) fn interned_id(&self, interned: &[(usize, MappingId)], mapping: &Arc<Mapping>) -> MappingId {
        let ptr = Arc::as_ptr(mapping) as usize;
        interned
            .iter()
            .find(|(known, _)| *known == ptr)
            .map_or_else(|| self.mapping_registry.intern(mapping), |(_, id)| *id)
    }

    /// Resolves one fragment whose mapping has already been interned.
    ///
    /// Taking the id as an argument is what lets a batch intern once per distinct mapping instead
    /// of once per fragment; the stored result is identical either way.
    fn resolve_with_mapping_id(&self, fragment: Mapped<Fragment>, mapping_id: MappingId) -> Result<()> {
        let (fragment, mapping) = fragment.into_parts();
        let observed_at = mapping.extract_observed_at(&self.resolver, fragment.source_data());
        let temporal = mapping.is_temporal();
        let (source_data, base_id, scope, parent_context) = fragment.into_parts();

        for (parent, property, child) in self.build_edges(&base_id, parent_context.as_ref(), temporal) {
            self.relationship_store.add_child(&parent, &property, &child)?;
        }

        self.entity_store.store_fragment(FragmentWrite {
            base_id,
            source_data,
            mapping_id,
            temporal,
            observed_at,
            scope,
            reads: mapping.source_reads(),
        })?;
        Ok(())
    }
}

impl FragmentSink for FragmentResolver {
    fn resolve(&self, fragment: Mapped<Fragment>) -> Result<()> {
        let mapping_id = self.mapping_registry.intern(fragment.mapping());
        self.resolve_with_mapping_id(fragment, mapping_id)
    }

    fn resolve_batch(&self, fragments: Vec<Mapped<Fragment>>) -> Vec<Result<()>> {
        self.resolve_batch_timed(fragments).0
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_source::EntitySource,
        entity_store::dashmap_latest_store::DashMapLatestEntityStore,
        fragment_resolver::FragmentResolver,
        fragment_sink::FragmentSink,
        relationship_store::dashmap_store::DashMapRelationshipStore,
    };
    use cassiopeia_ir::{
        fragment::Fragment,
        mapped::Mapped,
        parent_context::{ParentContext, ParentContextType},
        relationship_key::RelationshipKey,
        relationship_path::RelationshipPath,
    };
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use serde_json::json;
    use std::{path::Path, sync::Arc, time::Duration};
    use urn_rs::Urn;

    const SIMPLE: &str = r#"{
        version: "v4",
        dataModel: "AirQualityObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}" } },
    }"#;

    const SECOND_MODEL: &str = r#"{
        version: "v4",
        dataModel: "WeatherObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: { humidity: { source: "{{ humidity }}" } },
    }"#;

    const TEMPORAL: &str = r#"{
        version: "v4",
        dataModel: "AirQualityObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: {
            temperature: {
                source: "{{ temperature }}",
                properties: { observedAt: { source: "{{ timestamp }}" } },
            },
        },
    }"#;

    /// Loads every mapping into one runner so a single resolver serves them all, over a
    /// current-state store.
    fn latest_resolver(documents: &[&str]) -> (FragmentResolver, Vec<Arc<Mapping>>) {
        let mut runner = TemplateRunner::new();
        let mappings: Vec<Arc<Mapping>> = documents
            .iter()
            .map(|document| Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap()))
            .collect();
        let resolver = runner.resolver();
        let fragment_resolver = FragmentResolver::new(Box::new(DashMapLatestEntityStore::new()), Box::new(DashMapRelationshipStore::new()), resolver);
        (fragment_resolver, mappings)
    }

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    #[test]
    fn a_temporal_mapping_deduplicates_a_repeated_static_relationship_edge() {
        // A temporal mapping declaring a static relationship on every observation must record the edge
        // once, not once per observation, under the base-id key.
        let (resolver, mappings) = latest_resolver(&[TEMPORAL]);
        let station = urn("urn:ngsi-ld:Station:1");
        let site = urn("urn:ngsi-ld:Site:9");

        for timestamp in ["2026-04-03T22:00:20Z", "2026-04-03T22:05:20Z", "2026-04-03T22:10:20Z"] {
            let context = ParentContext::new(
                ParentContextType::Child(site.clone()),
                RelationshipKey::Path(RelationshipPath::flat(name("hasSite"))),
            );
            resolver
                .resolve(Mapped::new(
                    Fragment::new(json!({"temperature": 20, "timestamp": timestamp}), station.clone(), None, Some(vec![context])),
                    Arc::clone(&mappings[0]),
                ))
                .unwrap();
        }

        let mut units = resolver.assemble(&station).unwrap().units;
        assert_eq!(units.len(), 1);
        assert_eq!(
            units.remove(0).relationships().top_level().get(&name("hasSite")).map(Vec::as_slice),
            Some(&[site][..])
        );
    }

    #[test]
    fn resolve_batch_stores_every_fragment() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let batch = vec![
            Mapped::new(
                Fragment::new(json!({"t": 1}), urn("urn:ngsi-ld:Station:1"), None, None),
                Arc::clone(&mappings[0]),
            ),
            Mapped::new(
                Fragment::new(json!({"t": 2}), urn("urn:ngsi-ld:Station:2"), None, None),
                Arc::clone(&mappings[0]),
            ),
        ];

        let results = resolver.resolve_batch(batch);
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.get_unique_entity_count().unwrap(), 2);
    }

    #[test]
    fn a_batch_sharing_one_mapping_interns_the_id_that_direct_interning_would_hand_out() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let batch: Vec<Mapped<Fragment>> = (0..16)
            .map(|index| {
                Mapped::new(
                    Fragment::new(json!({"t": index}), urn(&format!("urn:ngsi-ld:Station:{index}")), None, None),
                    Arc::clone(&mappings[0]),
                )
            })
            .collect();

        let results = resolver.resolve_batch(batch);

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.mapping_registry.len(), 1);
        // Interning the same `Arc` again must return the id the batch already assigned it.
        let id = resolver.mapping_registry.intern(&mappings[0]);
        assert_eq!(resolver.mapping_registry.len(), 1);
        assert!(Arc::ptr_eq(&resolver.mapping_registry.resolve(id).unwrap(), &mappings[0]));
    }

    #[test]
    fn a_batch_mixing_two_mappings_stores_each_fragment_under_its_own_id() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE, SECOND_MODEL]);
        let air = urn("urn:ngsi-ld:AirQualityObserved:1");
        let weather = urn("urn:ngsi-ld:WeatherObserved:1");
        let batch = vec![
            Mapped::new(Fragment::new(json!({"t": 1}), air.clone(), None, None), Arc::clone(&mappings[0])),
            Mapped::new(Fragment::new(json!({"h": 2}), weather.clone(), None, None), Arc::clone(&mappings[1])),
            Mapped::new(Fragment::new(json!({"t": 3}), air.clone(), None, None), Arc::clone(&mappings[0])),
        ];

        let results = resolver.resolve_batch(batch);

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.mapping_registry.len(), 2);
        let first = resolver.mapping_registry.intern(&mappings[0]);
        let second = resolver.mapping_registry.intern(&mappings[1]);
        assert_ne!(first, second);
        // Each id reads back through the mapping its own fragments were resolved under.
        assert!(Arc::ptr_eq(&resolver.assemble(&air).unwrap().units[0].fragments()[0].1, &mappings[0]));
        assert!(Arc::ptr_eq(&resolver.assemble(&weather).unwrap().units[0].fragments()[0].1, &mappings[1]));
    }

    #[test]
    fn mapping_ids_stay_stable_across_two_successive_batches() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let batch = |offset: usize| -> Vec<Mapped<Fragment>> {
            (offset..offset + 4)
                .map(|index| {
                    Mapped::new(
                        Fragment::new(json!({"t": index}), urn(&format!("urn:ngsi-ld:Station:{index}")), None, None),
                        Arc::clone(&mappings[0]),
                    )
                })
                .collect()
        };

        resolver.resolve_batch(batch(0));
        let first = resolver.mapping_registry.intern(&mappings[0]);
        resolver.resolve_batch(batch(4));
        let second = resolver.mapping_registry.intern(&mappings[0]);

        assert_eq!(first, second);
        assert_eq!(resolver.mapping_registry.len(), 1);
    }

    #[test]
    fn a_concurrent_store_batch_reports_the_time_it_spent() {
        // A dashmap run takes the concurrent path, so store-write timing must reflect the time the
        // batch actually took rather than an empty duration.
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let batch: Vec<Mapped<Fragment>> = (0..64)
            .map(|index| {
                Mapped::new(
                    Fragment::new(json!({"t": index}), urn(&format!("urn:ngsi-ld:Station:{index}")), None, None),
                    Arc::clone(&mappings[0]),
                )
            })
            .collect();

        let (results, timing) = resolver.resolve_batch_timed(batch);

        assert!(results.iter().all(Result::is_ok));
        assert!(timing.store_write() > Duration::ZERO);
    }
}

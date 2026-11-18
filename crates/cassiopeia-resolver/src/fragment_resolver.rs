use crate::{
    entity_store::{
        observations::{Observations, RecordRanking},
        record_timestamp::RecordTimestamp,
        retention::Retention,
        store::EntityStore,
        write_batch::WriteBatches,
    },
    error::{ResolverError, Result},
    fragment_sink::FragmentSink,
    mapping_id::MappingId,
    mapping_registry::MappingRegistry,
    observed_at_failures::ObservedAtFailures,
    relationship_store::{relationship_edge::RelationshipEdge, store::RelationshipStore},
    store_batch_failure::StoreBatchFailure,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::RandomState;
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{
    fragment::Fragment,
    mapped::Mapped,
    parent_context::{ParentContext, ParentContextType},
    source_position::SourcePosition,
};
use cassiopeia_mapping::{
    mapping::Mapping,
    observation_plan::{ObservationPlan, RenderedObservations},
    template::{resolver::TemplateResolver, run_variables::RunVariables},
};
use dashmap::{DashMap, mapref::entry::Entry};
use std::{sync::Arc, time::Duration};
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
/// current-state store ranks a temporal mapping's records, so each distinct `observedAt` template of
/// the mapping is read once per record and passed to it; a series store keeps every observation and
/// is passed none.
#[derive(Clone)]
pub struct FragmentResolver {
    pub(crate) entity_store: Box<dyn EntityStore>,
    pub(crate) relationship_store: Box<dyn RelationshipStore>,
    pub(crate) mapping_registry: Arc<MappingRegistry>,
    pub(crate) resolver: TemplateResolver,
    pub(crate) parallelism: Parallelism,
    write_strategy: StoreWriteStrategy,
    /// Which records the entity store keeps, which decides whether a record's observation times are
    /// read.
    pub(crate) retention: Retention,
    /// Deduplicates a temporal mapping's identical edges so a static relationship declared on every
    /// observation does not accumulate one duplicate per observation under the base-id key. Each edge
    /// is kept with the earliest position it was given at so far, since the fragments of a batch
    /// arrive in whatever order their threads finish them.
    temporal_edge_dedup: Arc<DashMap<RelationshipEdge, SourcePosition, RandomState>>,
    /// The first failure of each mapping's `observedAt` template, for reporting the records a
    /// current-state store discards because of it.
    pub(crate) observed_at_failures: Arc<ObservedAtFailures>,
    /// Numbers the batches the resolver writes, in the order it writes them.
    pub(crate) write_batches: Arc<WriteBatches>,
}

impl FragmentResolver {
    /// Creates a resolver over the given stores and the shared template resolver.
    ///
    /// The resolver is used to read each fragment's `observedAt` value; it is the same engine the
    /// expander compiled its templates against.
    #[must_use]
    pub fn new(entity_store: Box<dyn EntityStore>, relationship_store: Box<dyn RelationshipStore>, resolver: TemplateResolver) -> FragmentResolver {
        let write_strategy = entity_store.write_strategy().combine(relationship_store.write_strategy());
        let retention = entity_store.retention();

        FragmentResolver {
            entity_store,
            relationship_store,
            mapping_registry: Arc::new(MappingRegistry::new()),
            resolver,
            parallelism: Parallelism::Parallel,
            write_strategy,
            retention,
            temporal_edge_dedup: Arc::new(DashMap::with_hasher(RandomState::new())),
            observed_at_failures: Arc::new(ObservedAtFailures::new()),
            write_batches: Arc::new(WriteBatches::new()),
        }
    }

    /// How a record of the mapping `mapping_id`, ranked as `plan` ranks it, ranks against the
    /// mapping's other records, given what its `observedAt` templates rendered, or `None` when it is
    /// not ranked.
    ///
    /// A failed template ranks the record as carrying no instant under it, and its failure is kept
    /// for the report that names the records a current-state store releases because of it.
    pub(crate) fn record_ranking(&self, mapping_id: MappingId, plan: &ObservationPlan, observed: Option<RenderedObservations>) -> RecordRanking {
        observed.map_or(RecordRanking::Unranked, |rendered| {
            let timestamps = rendered.into_iter().map(|rendered| match rendered {
                Ok(value) => RecordTimestamp::read(value),
                Err(error) => {
                    self.observed_at_failures.record(mapping_id, error);
                    RecordTimestamp::Unresolvable
                }
            });
            RecordRanking::Ranked(Observations::of(timestamps, plan.slot_templates()))
        })
    }

    /// Registers `mapping` read with `variables` before any of its fragments arrive.
    ///
    /// A mapping's id is handed out when it is first met, and the mappings joined onto one entity
    /// are ordered by id. The inputs of a run stream their fragments in concurrently, so the run
    /// registers every input's mappings up front, in the order of the manifest, to give them ids, and
    /// the joined entities their order, in that order on every run.
    pub fn register(&self, mapping: &Arc<Mapping>, variables: &RunVariables) {
        self.mapping_registry.intern(mapping, variables);
    }

    /// Sets whether batch preparation runs across threads.
    #[must_use]
    pub const fn with_parallelism(mut self, parallelism: Parallelism) -> FragmentResolver {
        self.parallelism = parallelism;
        self
    }

    /// Builds the parent-child edges the fragment at `position` records, keyed on base URNs, in the
    /// order its record names their targets.
    ///
    /// A temporal mapping re-declares its static relationships on every observation, so an identical
    /// edge from a temporal mapping is written only when no fragment earlier in the source has given
    /// it yet, and every later repeat is dropped before it reaches the store. A fragment that comes
    /// earlier in the source than the repeat written before it writes the edge again, so the store
    /// reads the target back at its earliest place. A non-temporal mapping writes every edge, and the
    /// store reads each target back once.
    pub(crate) fn build_edges(
        &self,
        base_urn: &Urn,
        parent_contexts: Option<&Vec<ParentContext>>,
        temporal: bool,
        position: SourcePosition,
    ) -> Vec<RelationshipEdge> {
        let Some(parent_contexts) = parent_contexts else {
            return Vec::new();
        };

        let mut edges = Vec::with_capacity(parent_contexts.len());
        for parent_context in parent_contexts {
            // The store key is the relationship key's text form: a bare name for a top-level
            // relationship, dotted segments for a nested one, and `name#index` for one instance of a
            // multi-attribute relationship, so the store groups each instance's objects apart.
            let property = parent_context.key().to_string();
            let (parent, child) = match parent_context.urn() {
                ParentContextType::Parent(parent) => (parent.clone(), base_urn.clone()),
                ParentContextType::Child(child) => (base_urn.clone(), child.clone()),
            };
            let edge = RelationshipEdge { parent, property, child };

            if temporal && !self.is_earliest_repeat(&edge, position) {
                continue;
            }
            edges.push(edge);
        }
        edges
    }

    /// Whether `edge`, which a temporal mapping gives at `position`, comes earlier in the source than
    /// every repeat of it given so far, keeping `position` as the earliest when it does.
    fn is_earliest_repeat(&self, edge: &RelationshipEdge, position: SourcePosition) -> bool {
        // A repeat, which every observation after the first is, is looked up without copying the
        // edge into a key.
        if let Some(mut earliest) = self.temporal_edge_dedup.get_mut(edge) {
            return Self::keep_if_earlier(&mut earliest, position);
        }
        // The map owns its key, while the fragment keeps the edge to write.
        match self.temporal_edge_dedup.entry(edge.clone()) {
            Entry::Occupied(mut earliest) => Self::keep_if_earlier(earliest.get_mut(), position),
            Entry::Vacant(first) => {
                first.insert(position);
                true
            }
        }
    }

    /// Whether `position` comes before `earliest`, which it then replaces.
    fn keep_if_earlier(earliest: &mut SourcePosition, position: SourcePosition) -> bool {
        let earlier = position < *earliest;
        if earlier {
            *earliest = position;
        }
        earlier
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
    /// (the private `prepare_fragments`, in [`coalesced_batch`](crate::coalesced_batch)), then flushes the relationship and
    /// entity stores once each. This turns an O(N) disk-backed run into O(batches) round-trips without
    /// changing single-fragment semantics. Stores that take concurrent writes take the concurrent
    /// path instead ([`concurrent_batch`](crate::concurrent_batch)), which writes the fragments of
    /// different entities in parallel and each entity's in the order of the batch.
    ///
    /// The results hold one entry per fragment.
    #[must_use]
    pub fn resolve_batch_timed(&self, fragments: Vec<Mapped<Fragment>>) -> (Vec<Result<()>>, ResolveBatchTiming) {
        if self.write_strategy == StoreWriteStrategy::Concurrent {
            return self.resolve_batch_concurrent(fragments);
        }

        let (prepared, preparation) = self.prepare_fragments(fragments);
        let count = prepared.len();
        let (rel_failure, relationship_write) = self.write_relationship_edges(&prepared);
        let (ent_failure, entity_write) = self.write_entity_fragments(prepared, self.write_batches.next());

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

    /// Interns every distinct pair of mapping and run variables in `readings`, keyed by the
    /// addresses of both, and returns the resulting table.
    ///
    /// A run wires one `Arc<Mapping>` per mapping file and one set of variables per input, and clones
    /// both onto every fragment an input produces, so the table holds a single entry in practice and
    /// a linear scan beats any map. Interning once per batch instead of once per fragment is what
    /// keeps every worker off the registry's one matching shard.
    pub(crate) fn intern_distinct<'a>(&self, readings: impl Iterator<Item = (&'a Arc<Mapping>, &'a RunVariables)>) -> Vec<((usize, usize), MappingId)> {
        let mut interned: Vec<((usize, usize), MappingId)> = Vec::new();
        for (mapping, variables) in readings {
            let key = Self::reading_key(mapping, variables);
            if interned.iter().any(|(known, _)| *known == key) {
                continue;
            }
            interned.push((key, self.mapping_registry.intern(mapping, variables)));
        }
        interned
    }

    /// Reads the id of a mapping read with `variables` out of a table built by
    /// [`intern_distinct`](FragmentResolver::intern_distinct), interning through the registry for a
    /// pair the table does not cover.
    pub(crate) fn interned_id(&self, interned: &[((usize, usize), MappingId)], mapping: &Arc<Mapping>, variables: &RunVariables) -> MappingId {
        let key = Self::reading_key(mapping, variables);
        interned
            .iter()
            .find(|(known, _)| *known == key)
            .map_or_else(|| self.mapping_registry.intern(mapping, variables), |(_, id)| *id)
    }

    /// The addresses a mapping and the run variables it reads a fragment with are told apart by.
    fn reading_key(mapping: &Arc<Mapping>, variables: &RunVariables) -> (usize, usize) {
        (Arc::as_ptr(mapping) as usize, variables.address())
    }
}

impl FragmentSink for FragmentResolver {
    fn resolve(&self, fragment: Mapped<Fragment>) -> Result<()> {
        let mapping_id = self.mapping_registry.intern(fragment.mapping(), fragment.inner().variables());
        self.write_prepared(self.prepare(fragment, mapping_id), self.write_batches.next())
    }

    fn resolve_batch(&self, fragments: Vec<Mapped<Fragment>>) -> Vec<Result<()>> {
        self.resolve_batch_timed(fragments).0
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_source::EntitySource,
        entity_store::{
            dashmap_latest_store::DashMapLatestEntityStore,
            observations::{Observations, RecordRanking},
            record_timestamp::RecordTimestamp,
            redb_latest_store::RedbLatestEntityStore,
            store::EntityStore,
        },
        fragment_resolver::FragmentResolver,
        fragment_sink::FragmentSink,
        mapping_id::MappingId,
        observed_at_failures::tests::failure,
        relationship_store::{dashmap_store::DashMapRelationshipStore, redb_store::RedbRelationshipStore, store::RelationshipStore},
        test_order::next_position,
    };
    use cassiopeia_ir::{
        fragment::Fragment,
        instance_index::InstanceIndex,
        mapped::Mapped,
        parent_context::{ParentContext, ParentContextType},
        relationship_key::RelationshipKey,
        relationship_path::RelationshipPath,
        relationships::InstanceObjects,
        source_position::SourcePosition,
    };
    use cassiopeia_mapping::{
        mapping::Mapping,
        observed_at::ObservedAt,
        template::{run_variables::RunVariables, runner::TemplateRunner},
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::{Map, json};
    use smallvec::smallvec;
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
                    Fragment::builder()
                        .source_data(json!({"temperature": 20, "timestamp": timestamp}))
                        .target_urn(station.clone())
                        .position(next_position())
                        .parent_context(Some(vec![context]))
                        .build(),
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

    /// How many times each pair of stores resolves the same batch, so an order that changed between
    /// runs would show.
    const RUNS: usize = 20;

    /// How many records an interleaved batch holds: enough to spread one batch over many threads.
    const RECORDS: usize = 400;

    /// The stores a run pairs on each entity store.
    fn store_pairs() -> [(Box<dyn EntityStore>, Box<dyn RelationshipStore>); 2] {
        [
            (Box::new(DashMapLatestEntityStore::new()), Box::new(DashMapRelationshipStore::new())),
            (Box::new(RedbLatestEntityStore::new().unwrap()), Box::new(RedbRelationshipStore::new().unwrap())),
        ]
    }

    /// The station every third record of an interleaved batch describes.
    fn station() -> Urn {
        urn("urn:ngsi-ld:Station:S1")
    }

    /// Resolves [`RECORDS`] records, in the order of the source, as one batch through `document` over
    /// `stores`: every third record describes [`station`], numbered among its records as the
    /// argument `links` is given, and the others describe stations of their own.
    fn resolve_interleaved(
        stores: (Box<dyn EntityStore>, Box<dyn RelationshipStore>),
        document: &str,
        links: impl Fn(usize) -> Vec<ParentContext>,
    ) -> FragmentResolver {
        let mut runner = TemplateRunner::new();
        let mapping = Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap());
        let (entity_store, relationship_store) = stores;
        let resolver = FragmentResolver::new(entity_store, relationship_store, runner.resolver());
        let first = SourcePosition::first_of(0);
        let batch = (0..RECORDS)
            .map(|record| {
                let target = if record % 3 == 0 {
                    station()
                } else {
                    urn(&format!("urn:ngsi-ld:Station:other-{record}"))
                };
                let fragment = Fragment::builder()
                    .source_data(json!({"temperature": record, "timestamp": "2026-04-03T22:00:20Z"}))
                    .target_urn(target)
                    .position(first.advanced(record))
                    .parent_context(Some(links(record / 3)))
                    .build();
                Mapped::new(fragment, Arc::clone(&mapping))
            })
            .collect();
        assert!(resolver.resolve_batch(batch).iter().all(Result::is_ok));
        resolver
    }

    /// The child links `targets` under `key`, in the order given.
    fn links_to(key: &RelationshipKey, targets: impl IntoIterator<Item = String>) -> Vec<ParentContext> {
        targets
            .into_iter()
            .map(|target| ParentContext::new(ParentContextType::Child(urn(&target)), key.clone()))
            .collect()
    }

    /// Every target `targets` gives the records of [`station`], in the order of the source, each once
    /// at its first place.
    fn in_source_order(targets: impl Fn(usize) -> Vec<String>) -> Vec<Urn> {
        let mut ordered: Vec<Urn> = Vec::new();
        for target in (0..RECORDS.div_ceil(3)).flat_map(targets) {
            let target = urn(&target);
            if !ordered.contains(&target) {
                ordered.push(target);
            }
        }
        ordered
    }

    #[test]
    fn the_targets_several_records_give_one_entity_follow_the_source_in_every_run_and_store() {
        let served_by = RelationshipKey::Path(RelationshipPath::flat(name("servedBy")));
        let building = RelationshipKey::Path(RelationshipPath::flat(name("building")));
        let airports = RelationshipKey::Instance {
            attribute: name("servesAirports"),
            index: InstanceIndex::from(1),
        };
        let character = RelationshipPath::flat(name("hasLeadActor")).push(name("playsCharacter"));
        // Each record names targets some earlier record named too, in another order.
        let served_lines = |record: usize| {
            vec![
                format!("urn:ngsi-ld:Line:L{}", record * 7 % 11),
                format!("urn:ngsi-ld:Line:L{}", record * 3 % 11),
            ]
        };
        let buildings = |record: usize| vec![format!("urn:ngsi-ld:Building:B{}", record % 4)];
        let airport = |record: usize| vec![format!("urn:ngsi-ld:Airport:A{}", record * 5 % 7)];
        let characters = |record: usize| vec![format!("urn:ngsi-ld:Character:C{}", record * 2 % 9)];
        let record_links = |record: usize| {
            let mut given = links_to(&served_by, served_lines(record));
            given.extend(links_to(&building, buildings(record)));
            given.extend(links_to(&airports, airport(record)));
            given.extend(links_to(&RelationshipKey::Path(character.clone()), characters(record)));
            given
        };

        for _ in 0..RUNS {
            for stores in store_pairs() {
                let resolver = resolve_interleaved(stores, SIMPLE, record_links);

                let units = resolver.assemble(&station()).unwrap().units;
                assert_eq!(units.len(), 1);
                let relationships = units[0].relationships();
                assert_eq!(relationships.top_level().get(&name("servedBy")), Some(&in_source_order(served_lines)));
                assert_eq!(relationships.top_level().get(&name("building")), Some(&in_source_order(buildings)));
                assert_eq!(
                    relationships.instances().as_ref().and_then(|instances| instances.get(&name("servesAirports"))),
                    Some(&InstanceObjects::from([(InstanceIndex::from(1), in_source_order(airport))]))
                );
                assert_eq!(
                    relationships.nested().as_ref().and_then(|nested| nested.get(&character)),
                    Some(&in_source_order(characters))
                );
            }
        }
    }

    #[test]
    fn a_temporal_mapping_s_repeated_target_stands_at_its_earliest_record_in_every_run_and_store() {
        let site = RelationshipKey::Path(RelationshipPath::flat(name("hasSite")));
        // The records alternate between two sites, so the first site stands first only when the
        // repeat kept is the earliest, not the first to reach the resolver.
        let sites = |record: usize| vec![format!("urn:ngsi-ld:Site:T{}", record % 2)];

        for _ in 0..RUNS {
            for stores in store_pairs() {
                let resolver = resolve_interleaved(stores, TEMPORAL, |record| links_to(&site, sites(record)));

                let mut units = resolver.assemble(&station()).unwrap().units;
                assert_eq!(units.len(), 1);
                assert_eq!(
                    units.remove(0).relationships().top_level().get(&name("hasSite")),
                    Some(&vec![urn("urn:ngsi-ld:Site:T0"), urn("urn:ngsi-ld:Site:T1")])
                );
            }
        }
    }

    #[test]
    fn a_temporal_repeat_earlier_in_the_source_than_the_one_written_is_written_again_and_a_later_one_is_not() {
        let (resolver, _) = latest_resolver(&[TEMPORAL]);
        let site = vec![ParentContext::new(
            ParentContextType::Child(urn("urn:ngsi-ld:Site:9")),
            RelationshipKey::Path(RelationshipPath::flat(name("hasSite"))),
        )];
        let station = station();
        let first = SourcePosition::first_of(0);

        let edges_at = |record: usize| resolver.build_edges(&station, Some(&site), true, first.advanced(record)).len();

        assert_eq!(
            [edges_at(5), edges_at(7), edges_at(2), edges_at(3), edges_at(0), edges_at(2)],
            [1, 0, 1, 0, 1, 0]
        );
        // A mapping without `observedAt` writes every repeat, and the store collapses them.
        assert_eq!(resolver.build_edges(&station, Some(&site), false, first.advanced(9)).len(), 1);
    }

    #[test]
    fn a_record_ranks_by_what_its_observed_at_templates_rendered_and_a_failure_is_kept_for_the_report() {
        let (resolver, mappings) = latest_resolver(&[TEMPORAL]);
        let mapping_id = MappingId::new(1);

        let plan = mappings[0].observation_plan();

        assert_eq!(resolver.record_ranking(mapping_id, plan, None), RecordRanking::Unranked);
        assert_eq!(
            resolver.record_ranking(mapping_id, plan, Some(smallvec![Ok(json!("soon"))])),
            RecordRanking::Ranked(Observations::of([RecordTimestamp::Unreadable(ObservedAt::new("soon"))], &[]))
        );
        assert!(resolver.observed_at_failures.take(mapping_id).is_none());
        assert_eq!(
            resolver.record_ranking(mapping_id, plan, Some(smallvec![Err(failure("{{ ts | trim }}"))])),
            RecordRanking::Ranked(Observations::of([RecordTimestamp::Unresolvable], &[]))
        );
        assert!(resolver.observed_at_failures.take(mapping_id).is_some());
    }

    #[test]
    fn resolve_batch_stores_every_fragment() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let batch = vec![
            Mapped::new(
                Fragment::builder()
                    .source_data(json!({"t": 1}))
                    .target_urn(urn("urn:ngsi-ld:Station:1"))
                    .position(next_position())
                    .build(),
                Arc::clone(&mappings[0]),
            ),
            Mapped::new(
                Fragment::builder()
                    .source_data(json!({"t": 2}))
                    .target_urn(urn("urn:ngsi-ld:Station:2"))
                    .position(next_position())
                    .build(),
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
                    Fragment::builder()
                        .source_data(json!({"t": index}))
                        .target_urn(urn(&format!("urn:ngsi-ld:Station:{index}")))
                        .position(next_position())
                        .build(),
                    Arc::clone(&mappings[0]),
                )
            })
            .collect();

        let results = resolver.resolve_batch(batch);

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.mapping_registry.len(), 1);
        // Interning the same `Arc` again must return the id the batch already assigned it.
        let id = resolver.mapping_registry.intern(&mappings[0], &RunVariables::none());
        assert_eq!(resolver.mapping_registry.len(), 1);
        assert!(Arc::ptr_eq(resolver.mapping_registry.resolve(id).unwrap().mapping(), &mappings[0]));
    }

    #[test]
    fn a_batch_mixing_two_mappings_stores_each_fragment_under_its_own_id() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE, SECOND_MODEL]);
        let air = urn("urn:ngsi-ld:AirQualityObserved:1");
        let weather = urn("urn:ngsi-ld:WeatherObserved:1");
        let batch = vec![
            Mapped::new(
                Fragment::builder()
                    .source_data(json!({"t": 1}))
                    .target_urn(air.clone())
                    .position(next_position())
                    .build(),
                Arc::clone(&mappings[0]),
            ),
            Mapped::new(
                Fragment::builder()
                    .source_data(json!({"h": 2}))
                    .target_urn(weather.clone())
                    .position(next_position())
                    .build(),
                Arc::clone(&mappings[1]),
            ),
            Mapped::new(
                Fragment::builder()
                    .source_data(json!({"t": 3}))
                    .target_urn(air.clone())
                    .position(next_position())
                    .build(),
                Arc::clone(&mappings[0]),
            ),
        ];

        let results = resolver.resolve_batch(batch);

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.mapping_registry.len(), 2);
        let first = resolver.mapping_registry.intern(&mappings[0], &RunVariables::none());
        let second = resolver.mapping_registry.intern(&mappings[1], &RunVariables::none());
        assert_ne!(first, second);
        // Each id reads back through the mapping its own fragments were resolved under.
        assert!(Arc::ptr_eq(resolver.assemble(&air).unwrap().units[0].fragments()[0].mapping(), &mappings[0]));
        assert!(Arc::ptr_eq(
            resolver.assemble(&weather).unwrap().units[0].fragments()[0].mapping(),
            &mappings[1]
        ));
    }

    /// Run variables setting `network` to `value`.
    fn network(value: &str) -> RunVariables {
        let mut variables = Map::new();
        variables.insert("network".to_string(), json!(value));
        RunVariables::new(variables)
    }

    /// One fragment of the station `id`, read with `variables`, under `mapping`.
    fn station_fragment(id: &Urn, record: serde_json::Value, variables: RunVariables, mapping: &Arc<Mapping>) -> Mapped<Fragment> {
        let fragment = Fragment::builder()
            .source_data(record)
            // Every fragment owns its target id, while the test keeps it to assemble the entity.
            .target_urn(id.clone())
            .position(next_position())
            .variables(variables)
            .build();
        Mapped::new(fragment, Arc::clone(mapping))
    }

    #[test]
    fn inputs_sharing_a_mapping_but_not_their_variables_keep_their_records_apart() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let station = urn("urn:ngsi-ld:Station:1");
        let batch = vec![
            station_fragment(&station, json!({"t": 1}), network("bikes"), &mappings[0]),
            station_fragment(&station, json!({"t": 2}), network("scooters"), &mappings[0]),
        ];

        assert!(resolver.resolve_batch(batch).iter().all(Result::is_ok));

        let assembled = resolver.assemble(&station).unwrap();
        // Each record and its variables are copied out of the assembly to be compared in a fixed order.
        let mut variables: Vec<_> = assembled.units[0]
            .fragments()
            .iter()
            .map(|fragment| (fragment.record().clone(), fragment.variables().as_value().clone()))
            .collect();
        variables.sort_by_key(|(record, _)| record.to_string());
        assert_eq!(
            variables,
            [
                (json!({"t": 1}), json!({"network": "bikes"})),
                (json!({"t": 2}), json!({"network": "scooters"})),
            ]
        );
    }

    #[test]
    fn inputs_sharing_a_mapping_and_equal_variables_merge_their_records() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let station = urn("urn:ngsi-ld:Station:1");
        let batch = vec![
            station_fragment(&station, json!({"t": 1}), network("bikes"), &mappings[0]),
            station_fragment(&station, json!({"t": 1}), network("bikes"), &mappings[0]),
        ];

        assert!(resolver.resolve_batch(batch).iter().all(Result::is_ok));

        assert_eq!(resolver.mapping_registry.len(), 1);
        assert_eq!(resolver.assemble(&station).unwrap().units[0].fragments().len(), 1);
    }

    #[test]
    fn mapping_ids_stay_stable_across_two_successive_batches() {
        let (resolver, mappings) = latest_resolver(&[SIMPLE]);
        let batch = |offset: usize| -> Vec<Mapped<Fragment>> {
            (offset..offset + 4)
                .map(|index| {
                    Mapped::new(
                        Fragment::builder()
                            .source_data(json!({"t": index}))
                            .target_urn(urn(&format!("urn:ngsi-ld:Station:{index}")))
                            .position(next_position())
                            .build(),
                        Arc::clone(&mappings[0]),
                    )
                })
                .collect()
        };

        resolver.resolve_batch(batch(0));
        let first = resolver.mapping_registry.intern(&mappings[0], &RunVariables::none());
        resolver.resolve_batch(batch(4));
        let second = resolver.mapping_registry.intern(&mappings[0], &RunVariables::none());

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
                    Fragment::builder()
                        .source_data(json!({"t": index}))
                        .target_urn(urn(&format!("urn:ngsi-ld:Station:{index}")))
                        .position(next_position())
                        .build(),
                    Arc::clone(&mappings[0]),
                )
            })
            .collect();

        let (results, timing) = resolver.resolve_batch_timed(batch);

        assert!(results.iter().all(Result::is_ok));
        assert!(timing.store_write() > Duration::ZERO);
    }
}

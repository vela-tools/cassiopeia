//! Reading stored fragments back out as assembled entities.
//!
//! This is the [`EntitySource`] half of [`FragmentResolver`]: everything the write path put into the
//! stores comes back through here, joined per base id and streamed out in id order. The two halves
//! are separate concerns that change for unrelated reasons, so they live apart.

use crate::{
    entity_source::EntitySource,
    entity_store::store::{AssembledFragments as StoredUnits, StoredUnit},
    error::{ResolverError, Result},
    fragment_resolver::FragmentResolver,
    mapping_id::MappingId,
};
use cassiopeia_ir::{
    assembled_entity::{AssembledEntity, AssembledFragments},
    relationship_path::RelationshipPath,
    relationships::{NestedRelationships, Relationships},
};
use cassiopeia_mapping::mapping::Mapping;
use rayon::prelude::*;
use smallvec::SmallVec;
use std::{
    mem,
    ops::ControlFlow,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::trace;
use urn_rs::Urn;

/// One base id's relationships split into its flat (top-level) map and its nested-relationship map.
type SplitRelationships = (Relationships, Option<NestedRelationships>);

/// Every distinct mapping one base id's stored fragments were produced by, paired with its id.
///
/// One entry in the common case, at most one per mapping contributing to the id, so a linear scan
/// beats any map and the inline capacity keeps the table off the heap.
type MappingTable = SmallVec<[(MappingId, Arc<Mapping>); 1]>;

/// What one assembly scan spent its time on.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssemblyTiming {
    /// Wall time spent assembling chunks, summed across the scan.
    ///
    /// The assembly runs on rayon workers, so this is the driving thread's view of how long the
    /// parallel chunks took, not the CPU time they consumed. A per-thread CPU clock read on the
    /// driving thread cannot see the workers, which is why the assembler stage reports service time
    /// with no CPU time beside it.
    pub assembly: Duration,
}

impl FragmentResolver {
    /// Splits the stored relationship edges of one base id into its flat and nested maps.
    fn split_relationships(&self, base_id: &Urn) -> Result<SplitRelationships> {
        let stored_relationships = self.relationship_store.get_all_relationships(base_id)?;
        let mut relationships = Relationships::default();
        let mut nested_relationships = NestedRelationships::default();
        for (key, targets) in stored_relationships {
            match key.parse::<RelationshipPath>()? {
                RelationshipPath::Flat(name) => {
                    relationships.insert(name, targets);
                }
                nested @ RelationshipPath::Nested(_) => {
                    nested_relationships.insert(nested, targets);
                }
            }
        }
        let nested = if nested_relationships.is_empty() { None } else { Some(nested_relationships) };
        Ok((relationships, nested))
    }

    /// Resolves each distinct mapping id across one base id's stored units into its mapping.
    ///
    /// A series id repeats a single mapping id across every observation it holds, so resolving per
    /// fragment would take the registry's one matching shard read lock once per observation, from
    /// every rayon worker at once. Resolving per distinct id instead makes that one probe per id.
    /// An id the registry never handed out is a corrupt store, reported as the entity having no
    /// configuration.
    fn resolve_unit_mappings(&self, units: &[StoredUnit], base_id: &Urn) -> Result<MappingTable> {
        let mut resolved = MappingTable::new();
        for stored in units.iter().flatten() {
            if resolved.iter().any(|(known, _)| *known == stored.mapping_id) {
                continue;
            }
            let mapping = self
                .mapping_registry
                .resolve(stored.mapping_id)
                .ok_or_else(|| ResolverError::MissingConfigForEntity { entity: base_id.clone() })?;
            resolved.push((stored.mapping_id, mapping));
        }
        Ok(resolved)
    }

    /// How many ids to assemble per chunk so that one chunk yields roughly `batch_size` emit-units.
    ///
    /// Chunking by id alone is wrong whenever an id carries many units: a series run holding a
    /// thousand observations per id would materialise a thousand times `batch_size` entities before
    /// emitting any of them, so nothing downstream starts until assembly has finished and peak memory
    /// grows to the whole assembled dataset. Sizing the chunk by the units it will produce is what
    /// makes phase 2 actually pipeline.
    fn id_chunk_size(&self, batch_size: usize) -> usize {
        let id_count = self.entity_store.count();
        let units_per_id = self.entity_store.unit_count().div_ceil(id_count.max(1));
        batch_size.div_ceil(units_per_id.max(1)).max(1)
    }
}

impl EntitySource for FragmentResolver {
    fn assemble(&self, base_id: &Urn) -> Result<Vec<AssembledEntity>> {
        let StoredUnits { scope, units } = self.entity_store.assemble_entity(base_id)?;
        if units.is_empty() {
            return Err(ResolverError::MissingConfigForEntity { entity: base_id.clone() });
        }

        // Assembly drains the store: `drive_assembly` visits each base id exactly once and destroys
        // the stores immediately afterwards, so nothing needs the id's fragments again. Each stored
        // relationship key is a path validated as an NGSI-LD name when stored, so re-validating on
        // the way back surfaces a corrupt key as an error.
        let (mut relationships, mut nested_relationships) = self.split_relationships(base_id)?;
        let mut scope = scope;

        // Resolved once per distinct mapping id rather than once per fragment: every observation of a
        // series id names the same mapping, and the registry's shard lock is shared by every worker.
        let resolved = self.resolve_unit_mappings(&units, base_id)?;

        let unit_count = units.len();
        let mut entities = Vec::with_capacity(unit_count);
        for (index, unit) in units.into_iter().enumerate() {
            let mut fragments: AssembledFragments = SmallVec::with_capacity(unit.len());
            for stored in unit {
                let mapping = resolved
                    .iter()
                    .find(|(known, _)| *known == stored.mapping_id)
                    .map(|(_, mapping)| Arc::clone(mapping))
                    .ok_or_else(|| ResolverError::MissingConfigForEntity { entity: base_id.clone() })?;
                fragments.push((stored.data, mapping));
            }

            // Relationships and scope render on every unit; a unit's mappings pick up only the
            // relationships they declare, so a static relationship renders on the mapping's own unit and
            // observation units of a temporal mapping ignore it. The last unit moves them rather than
            // cloning; earlier ones clone.
            let is_last = index + 1 == unit_count;
            let (unit_relationships, unit_nested, unit_scope) = if is_last {
                (mem::take(&mut relationships), nested_relationships.take(), scope.take())
            } else {
                (relationships.clone(), nested_relationships.clone(), scope.clone())
            };

            entities.push(AssembledEntity::new(base_id.clone(), unit_scope, unit_relationships, unit_nested, fragments));
        }

        trace!("Assembled entity {} into {} unit(s)", base_id, entities.len());
        Ok(entities)
    }

    fn get_entity_ids(&self) -> Result<Vec<Urn>> {
        Ok(self.entity_store.get_entity_ids()?)
    }

    fn get_unique_entity_count(&self) -> Result<u64> {
        Ok(u64::try_from(self.entity_store.count()).unwrap_or(u64::MAX))
    }

    fn get_emitted_count(&self) -> Result<u64> {
        Ok(u64::try_from(self.entity_store.unit_count()).unwrap_or(u64::MAX))
    }

    /// Assembly is parallel per chunk via rayon, then `emit` is called serially on the driving
    /// thread: emitting inside the rayon closure would deadlock, because rayon workers blocked on a
    /// full downstream channel saturate the global pool and starve the next stage's own `par_iter`.
    /// Collecting each chunk first keeps the workers pure-CPU and confines any back-pressure blocking
    /// to the driving thread. Each base id's units are emitted contiguously so the aggregator can
    /// group by id while holding one id at a time.
    fn drive_assembly(&self, batch_size: usize, emit: &mut dyn FnMut(Result<AssembledEntity>) -> ControlFlow<()>) -> Result<AssemblyTiming> {
        let id_chunk = self.id_chunk_size(batch_size);
        let mut assembly = Duration::ZERO;
        let mut broken = false;

        self.entity_store.for_each_entity_id_chunk(id_chunk, &mut |chunk: Vec<Urn>| {
            if broken {
                return Ok(());
            }
            let started = Instant::now();
            let assembled: Vec<Result<Vec<AssembledEntity>>> = chunk.par_iter().map(|urn| self.assemble(urn)).collect();
            assembly = assembly.saturating_add(started.elapsed());

            'chunk: for result in assembled {
                match result {
                    Ok(entities) => {
                        for entity in entities {
                            if emit(Ok(entity)).is_break() {
                                broken = true;
                                break 'chunk;
                            }
                        }
                    }
                    Err(error) => {
                        if emit(Err(error)).is_break() {
                            broken = true;
                            break 'chunk;
                        }
                    }
                }
            }
            Ok(())
        })?;

        Ok(AssemblyTiming { assembly })
    }

    fn destroy(&self) {
        let _ = self.entity_store.destroy();
        let _ = self.relationship_store.destroy();
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_source::EntitySource,
        entity_store::{
            dashmap_latest_store::DashMapLatestEntityStore,
            dashmap_series_store::DashMapSeriesEntityStore,
            store::{EntityStore, FragmentWrite},
        },
        error::ResolverError,
        fragment_resolver::FragmentResolver,
        fragment_sink::FragmentSink,
        mapping_id::MappingId,
        relationship_store::dashmap_store::DashMapRelationshipStore,
    };
    use cassiopeia_ir::{
        assembled_entity::AssembledEntity,
        fragment::Fragment,
        mapped::Mapped,
        parent_context::{ParentContext, ParentContextType},
        relationship_path::RelationshipPath,
    };
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use serde_json::json;
    use std::{ops::ControlFlow, path::Path, sync::Arc, time::Duration};
    use urn_rs::Urn;

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

    const SIMPLE: &str = r#"{
        version: "v4",
        dataModel: "AirQualityObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}" } },
    }"#;

    const OTHER: &str = r#"{
        version: "v4",
        dataModel: "AirQualityObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: { name: { source: "{{ name }}" } },
    }"#;

    /// Builds a resolver over `store` holding `document`'s mapping.
    fn resolver_over(store: Box<dyn EntityStore>, document: &str) -> (FragmentResolver, Arc<Mapping>) {
        let mut runner = TemplateRunner::new();
        let mapping = Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap());
        let resolver = runner.resolver();
        (FragmentResolver::new(store, Box::new(DashMapRelationshipStore::new()), resolver), mapping)
    }

    /// Stores `observations` observations of one id, then drives assembly recording the chunk each
    /// emitted unit arrived in.
    fn chunk_sizes(observations: usize, ids: usize, batch_size: usize) -> Vec<usize> {
        let (resolver, mapping) = resolver_over(Box::new(DashMapSeriesEntityStore::new()), TEMPORAL);
        let sink: &dyn FragmentSink = &resolver;
        for id in 0..ids {
            for observation in 0..observations {
                let urn: Urn = format!("urn:ngsi-ld:Station:{id}").parse().unwrap();
                let record = json!({ "temperature": observation, "timestamp": format!("2026-04-03T22:{observation:02}:00Z") });
                sink.resolve(Mapped::new(Fragment::new(record, urn, None, None), Arc::clone(&mapping))).unwrap();
            }
        }

        // Each id assembles atomically, so a chunk's emitted count is the number of ids it covered
        // times the units each carries; recording it shows how large a slice assembly materialised.
        let mut per_chunk: Vec<usize> = Vec::new();
        let mut seen = 0;
        let mut last_id: Option<Urn> = None;
        resolver
            .drive_assembly(batch_size, &mut |result| {
                let entity = result.unwrap();
                if last_id.as_ref() != Some(entity.id()) {
                    last_id = Some(entity.id().clone());
                    per_chunk.push(0);
                }
                if let Some(chunk) = per_chunk.last_mut() {
                    *chunk += 1;
                }
                seen += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(seen, observations * ids);
        per_chunk
    }

    #[test]
    fn a_series_id_with_many_units_still_emits_every_observation() {
        let per_id = chunk_sizes(50, 4, 100);

        assert_eq!(per_id.len(), 4);
        assert!(per_id.iter().all(|count| *count == 50));
    }

    #[test]
    fn the_id_chunk_shrinks_so_a_chunk_yields_about_one_batch_of_units() {
        let (resolver, mapping) = resolver_over(Box::new(DashMapSeriesEntityStore::new()), TEMPORAL);
        let sink: &dyn FragmentSink = &resolver;
        for id in 0..10 {
            for observation in 0..100 {
                let urn: Urn = format!("urn:ngsi-ld:Station:{id}").parse().unwrap();
                let record = json!({ "temperature": observation, "timestamp": format!("2026-04-03T22:{observation:02}:00Z") });
                sink.resolve(Mapped::new(Fragment::new(record, urn, None, None), Arc::clone(&mapping))).unwrap();
            }
        }

        // 1000 units across 10 ids is 100 units per id, so a 200-unit batch covers two ids at a time
        // rather than the whole store.
        assert_eq!(resolver.id_chunk_size(200), 2);
    }

    #[test]
    fn one_unit_per_id_leaves_the_chunk_at_the_full_batch_size() {
        let (resolver, mapping) = resolver_over(Box::new(DashMapLatestEntityStore::new()), SIMPLE);
        let sink: &dyn FragmentSink = &resolver;
        for id in 0..5 {
            let urn: Urn = format!("urn:ngsi-ld:Station:{id}").parse().unwrap();
            sink.resolve(Mapped::new(Fragment::new(json!({ "temperature": 20 }), urn, None, None), Arc::clone(&mapping)))
                .unwrap();
        }

        assert_eq!(resolver.id_chunk_size(10_000), 10_000);
    }

    #[test]
    fn an_empty_store_yields_a_usable_chunk_size_and_assembles_nothing() {
        let (resolver, _mapping) = resolver_over(Box::new(DashMapSeriesEntityStore::new()), TEMPORAL);

        assert!(resolver.id_chunk_size(10_000) >= 1);
        let mut emitted = 0;
        resolver
            .drive_assembly(10_000, &mut |_result| {
                emitted += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(emitted, 0);
    }

    #[test]
    fn the_scan_reports_the_time_its_chunks_took() {
        let (resolver, mapping) = resolver_over(Box::new(DashMapSeriesEntityStore::new()), TEMPORAL);
        let sink: &dyn FragmentSink = &resolver;
        for observation in 0..20 {
            let urn: Urn = "urn:ngsi-ld:Station:1".parse().unwrap();
            let record = json!({ "temperature": observation, "timestamp": format!("2026-04-03T22:{observation:02}:00Z") });
            sink.resolve(Mapped::new(Fragment::new(record, urn, None, None), Arc::clone(&mapping))).unwrap();
        }

        let timing = resolver.drive_assembly(8, &mut |_result| ControlFlow::Continue(())).unwrap();

        assert!(timing.assembly > Duration::ZERO);
    }

    /// Loads every mapping into one runner so a single resolver serves them all.
    fn resolver_with(store: Box<dyn EntityStore>, documents: &[&str]) -> (FragmentResolver, Vec<Arc<Mapping>>) {
        let mut runner = TemplateRunner::new();
        let mappings: Vec<Arc<Mapping>> = documents
            .iter()
            .map(|document| Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap()))
            .collect();
        let resolver = runner.resolver();
        let fragment_resolver = FragmentResolver::new(store, Box::new(DashMapRelationshipStore::new()), resolver);
        (fragment_resolver, mappings)
    }

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    /// The one assembled unit for a base id, asserting exactly one exists.
    fn only_unit(resolver: &FragmentResolver, id: &Urn) -> AssembledEntity {
        let mut units = resolver.assemble(id).unwrap();
        assert_eq!(units.len(), 1, "expected exactly one unit for {id}");
        units.remove(0)
    }

    #[test]
    fn resolve_then_assemble_returns_the_stored_data() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapLatestEntityStore::new()), &[SIMPLE]);
        let station = urn("urn:ngsi-ld:Station:1");
        let fragment = Fragment::new(json!({"temperature": 21.5}), station.clone(), None, None);

        resolver.resolve(Mapped::new(fragment, Arc::clone(&mappings[0]))).unwrap();

        let unit = only_unit(&resolver, &station);
        assert_eq!(unit.id().to_string(), "urn:ngsi-ld:Station:1");
        assert_eq!(unit.fragments().len(), 1);
        assert_eq!(unit.fragments()[0].0, json!({"temperature": 21.5}));
    }

    #[test]
    fn a_child_context_becomes_a_relationship_on_the_source_entity() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapLatestEntityStore::new()), &[SIMPLE]);
        let station = urn("urn:ngsi-ld:Station:1");
        let road = urn("urn:ngsi-ld:Road:9");
        let context = ParentContext::new(ParentContextType::Child(road.clone()), RelationshipPath::flat(name("refRoad")));
        let fragment = Fragment::new(json!({"t": 1}), station.clone(), None, Some(vec![context]));

        resolver.resolve(Mapped::new(fragment, Arc::clone(&mappings[0]))).unwrap();

        let unit = only_unit(&resolver, &station);
        assert_eq!(unit.relationships().get(&name("refRoad")).map(Vec::as_slice), Some(&[road][..]));
        assert!(unit.nested_relationships().is_none());
    }

    #[test]
    fn a_nested_child_context_becomes_a_nested_relationship() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapLatestEntityStore::new()), &[SIMPLE]);
        let movie = urn("urn:ngsi-ld:Movie:1");
        let character = urn("urn:ngsi-ld:Character:JackSparrow");
        let path = RelationshipPath::flat(name("hasLeadActor")).push(name("playsCharacter"));
        let context = ParentContext::new(ParentContextType::Child(character.clone()), path.clone());
        let fragment = Fragment::new(json!({"t": 1}), movie.clone(), None, Some(vec![context]));

        resolver.resolve(Mapped::new(fragment, Arc::clone(&mappings[0]))).unwrap();

        let unit = only_unit(&resolver, &movie);
        assert!(unit.relationships().is_empty());
        assert_eq!(
            unit.nested_relationships().as_ref().and_then(|map| map.get(&path)).map(Vec::as_slice),
            Some(&[character][..])
        );
    }

    #[test]
    fn two_mappings_targeting_one_id_join_into_one_unit_carrying_both() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapLatestEntityStore::new()), &[SIMPLE, OTHER]);
        let station = urn("urn:ngsi-ld:Station:1");

        resolver
            .resolve(Mapped::new(
                Fragment::new(json!({"temperature": 1}), station.clone(), None, None),
                Arc::clone(&mappings[0]),
            ))
            .unwrap();
        resolver
            .resolve(Mapped::new(
                Fragment::new(json!({"name": "s"}), station.clone(), None, None),
                Arc::clone(&mappings[1]),
            ))
            .unwrap();

        let unit = only_unit(&resolver, &station);
        assert_eq!(unit.fragments().len(), 2);
        let datas: Vec<&serde_json::Value> = unit.fragments().iter().map(|(data, _)| data).collect();
        assert!(datas.contains(&&json!({"temperature": 1})));
        assert!(datas.contains(&&json!({"name": "s"})));
    }

    #[test]
    fn temporal_records_for_one_id_keep_the_latest_in_current_state() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapLatestEntityStore::new()), &[TEMPORAL]);
        let base = urn("urn:ngsi-ld:Station:1");

        for (temperature, timestamp) in [(20, "2026-04-03T22:00:20Z"), (21, "2026-04-03T22:05:20Z")] {
            resolver
                .resolve(Mapped::new(
                    Fragment::new(json!({"temperature": temperature, "timestamp": timestamp}), base.clone(), None, None),
                    Arc::clone(&mappings[0]),
                ))
                .unwrap();
        }

        assert_eq!(resolver.get_entity_ids().unwrap(), vec![base.clone()]);
        let unit = only_unit(&resolver, &base);
        assert_eq!(unit.fragments()[0].0, json!({"temperature": 21, "timestamp": "2026-04-03T22:05:20Z"}));
    }

    #[test]
    fn a_series_store_emits_one_id_contiguous_unit_per_observation() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapSeriesEntityStore::new()), &[TEMPORAL]);
        let base = urn("urn:ngsi-ld:Station:1");

        for (temperature, timestamp) in [(20, "2026-04-03T22:00:20Z"), (21, "2026-04-03T22:05:20Z"), (22, "2026-04-03T22:10:20Z")] {
            resolver
                .resolve(Mapped::new(
                    Fragment::new(json!({"temperature": temperature, "timestamp": timestamp}), base.clone(), None, None),
                    Arc::clone(&mappings[0]),
                ))
                .unwrap();
        }

        assert_eq!(resolver.get_emitted_count().unwrap(), 3);
        assert_eq!(resolver.get_unique_entity_count().unwrap(), 1);
        let units = resolver.assemble(&base).unwrap();
        assert_eq!(units.len(), 3);
        for unit in &units {
            assert_eq!(unit.id(), &base);
            assert_eq!(unit.fragments().len(), 1);
        }
    }

    #[test]
    fn every_observation_of_a_series_id_resolves_to_the_one_registered_mapping() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapSeriesEntityStore::new()), &[TEMPORAL]);
        let base = urn("urn:ngsi-ld:Station:1");
        for (temperature, timestamp) in [(20, "2026-04-03T22:00:20Z"), (21, "2026-04-03T22:05:20Z"), (22, "2026-04-03T22:10:20Z")] {
            resolver
                .resolve(Mapped::new(
                    Fragment::new(json!({"temperature": temperature, "timestamp": timestamp}), base.clone(), None, None),
                    Arc::clone(&mappings[0]),
                ))
                .unwrap();
        }

        let units = resolver.assemble(&base).unwrap();

        assert_eq!(units.len(), 3);
        // Resolving the mapping once per distinct id must still hand every unit the very same
        // configuration the registry holds, not a copy of it.
        for unit in &units {
            assert_eq!(unit.fragments().len(), 1);
            assert!(Arc::ptr_eq(&unit.fragments()[0].1, &mappings[0]));
        }
    }

    #[test]
    fn a_joined_unit_gives_each_fragment_the_mapping_that_produced_it() {
        let (resolver, mappings) = resolver_with(Box::new(DashMapLatestEntityStore::new()), &[SIMPLE, OTHER]);
        let station = urn("urn:ngsi-ld:Station:1");
        resolver
            .resolve(Mapped::new(
                Fragment::new(json!({"temperature": 1}), station.clone(), None, None),
                Arc::clone(&mappings[0]),
            ))
            .unwrap();
        resolver
            .resolve(Mapped::new(
                Fragment::new(json!({"name": "s"}), station.clone(), None, None),
                Arc::clone(&mappings[1]),
            ))
            .unwrap();

        let unit = only_unit(&resolver, &station);

        assert_eq!(unit.fragments().len(), 2);
        for (data, mapping) in unit.fragments() {
            let expected = if *data == json!({"temperature": 1}) { &mappings[0] } else { &mappings[1] };
            assert!(Arc::ptr_eq(mapping, expected));
        }
    }

    #[test]
    fn a_fragment_carrying_an_unknown_mapping_id_reports_the_entity_as_unconfigured() {
        let (resolver, _mappings) = resolver_with(Box::new(DashMapSeriesEntityStore::new()), &[SIMPLE]);
        let station = urn("urn:ngsi-ld:Station:1");
        // Written straight to the store, so the id never passed through the registry.
        resolver
            .entity_store
            .store_fragment(FragmentWrite {
                base_id: station.clone(),
                source_data: json!({"temperature": 1}),
                mapping_id: MappingId::new(9999),
                temporal: false,
                observed_at: None,
                scope: None,
            })
            .unwrap();

        let error = resolver.assemble(&station).unwrap_err();

        assert!(matches!(error, ResolverError::MissingConfigForEntity { entity } if entity == station));
    }

    #[test]
    fn an_id_the_store_holds_nothing_for_reports_the_entity_as_unconfigured() {
        let (resolver, _mappings) = resolver_with(Box::new(DashMapSeriesEntityStore::new()), &[SIMPLE]);
        let station = urn("urn:ngsi-ld:Station:1");

        let error = resolver.assemble(&station).unwrap_err();

        assert!(matches!(error, ResolverError::MissingConfigForEntity { entity } if entity == station));
    }
}

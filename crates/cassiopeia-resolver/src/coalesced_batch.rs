//! The coalesced write path of [`FragmentResolver`]: a whole batch is prepared first, then flushed to
//! the relationship store and the entity store once each, which is what a transactional,
//! disk-backed store needs to stay at one round-trip per batch.

use crate::{
    entity_store::{error::EntityStoreError, store::FragmentWrite},
    fragment_resolver::FragmentResolver,
    mapping_id::MappingId,
    relationship_store::error::RelationshipStoreError,
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
use cassiopeia_mapping::{mapping::Mapping, observed_at::ObservedAt};
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use rayon::prelude::*;
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use urn_rs::Urn;

/// The per-fragment side-effects prepared before a coalesced store flush.
///
/// Every field is owned: the resolver is the fragment's last holder, so preparation takes the record
/// and the scope apart rather than borrowing them, and the store write below moves them into place.
pub(crate) struct PreparedFragment {
    base_id: Urn,
    source_data: Value,
    scope: Option<NgsiLdScope>,
    mapping: Arc<Mapping>,
    observed_at: Option<ObservedAt>,
    temporal: bool,
    edges: Vec<(Urn, String, Urn)>,
}

/// The owned half of one entity-store write, set apart from the mapping whose read set the write
/// borrows.
struct OwnedWrite {
    base_id: Urn,
    source_data: Value,
    mapping_id: MappingId,
    temporal: bool,
    observed_at: Option<ObservedAt>,
    scope: Option<NgsiLdScope>,
}

/// A prepared fragment paired with its interned mapping id, carried from preparation through both
/// store writes.
pub(crate) type PreparedSuccess = (PreparedFragment, MappingId);

impl FragmentResolver {
    /// Prepares each fragment's side-effects, interning its mapping, and returns the prepared
    /// fragments with their mapping ids and the preparation time.
    pub(crate) fn prepare_fragments(&self, fragments: Vec<Mapped<Fragment>>) -> (Vec<PreparedSuccess>, Duration) {
        let prepare_started = Instant::now();
        let prepare = |fragment: Mapped<Fragment>| -> PreparedFragment {
            let (fragment, mapping) = fragment.into_parts();
            let observed_at = mapping.extract_observed_at(&self.resolver, fragment.source_data());
            let temporal = mapping.is_temporal();
            let (source_data, base_id, scope, parent_context) = fragment.into_parts();
            let edges = self.build_edges(&base_id, parent_context.as_ref(), temporal);

            PreparedFragment {
                base_id,
                source_data,
                scope,
                mapping,
                observed_at,
                temporal,
                edges,
            }
        };

        let prepared: Vec<PreparedFragment> = match self.parallelism {
            Parallelism::Parallel => fragments.into_par_iter().map(prepare).collect(),
            Parallelism::Sequential => fragments.into_iter().map(prepare).collect(),
        };
        let preparation = prepare_started.elapsed();

        let interned = self.intern_distinct(prepared.iter().map(|prepared| &prepared.mapping));
        let successes = prepared
            .into_iter()
            .map(|prepared| {
                let mapping_id = self.interned_id(&interned, &prepared.mapping);
                (prepared, mapping_id)
            })
            .collect();

        (successes, preparation)
    }

    /// Writes every prepared relationship edge in one batch, returning any failure message and the
    /// time the write took.
    pub(crate) fn write_relationship_edges(&self, successes: &[PreparedSuccess]) -> (Option<RelationshipStoreError>, Duration) {
        let edge_refs: Vec<(Urn, &str, Urn)> = successes
            .iter()
            .flat_map(|(prepared, _)| {
                prepared
                    .edges
                    .iter()
                    .map(|(parent, property, child)| (parent.clone(), property.as_str(), child.clone()))
            })
            .collect();
        let relationship_started = Instant::now();
        let failure = if edge_refs.is_empty() {
            None
        } else {
            self.relationship_store.add_child_batch(&edge_refs).err()
        };
        (failure, relationship_started.elapsed())
    }

    /// Writes every prepared fragment to the entity store in one batch, carrying each interned
    /// mapping id, and returns any failure message and the time the write took.
    ///
    /// Consuming the prepared fragments is what lets each record move into the store instead of being
    /// copied there. Each write borrows its mapping's read set, so the mappings are set aside first
    /// and kept alive for the whole store call.
    pub(crate) fn write_entity_fragments(&self, successes: Vec<PreparedSuccess>) -> (Option<EntityStoreError>, Duration) {
        let (owned, mappings): (Vec<OwnedWrite>, Vec<Arc<Mapping>>) = successes
            .into_iter()
            .map(|(prepared, mapping_id)| {
                let PreparedFragment {
                    base_id,
                    source_data,
                    scope,
                    mapping,
                    observed_at,
                    temporal,
                    ..
                } = prepared;
                (
                    OwnedWrite {
                        base_id,
                        source_data,
                        mapping_id,
                        temporal,
                        observed_at,
                        scope,
                    },
                    mapping,
                )
            })
            .unzip();
        let writes: Vec<FragmentWrite<'_>> = owned
            .into_iter()
            .zip(&mappings)
            .map(|(write, mapping)| FragmentWrite {
                base_id: write.base_id,
                source_data: write.source_data,
                mapping_id: write.mapping_id,
                temporal: write.temporal,
                observed_at: write.observed_at,
                scope: write.scope,
                reads: mapping.source_reads(),
            })
            .collect();
        let entity_started = Instant::now();
        let failure = if writes.is_empty() {
            None
        } else {
            self.entity_store.store_fragment_batch(writes).err()
        };
        (failure, entity_started.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_source::EntitySource,
        entity_store::redb_latest_store::RedbLatestEntityStore,
        fragment_resolver::FragmentResolver,
        relationship_store::redb_store::RedbRelationshipStore,
    };
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
    use cassiopeia_mapping::{
        mapping::Mapping,
        source_reads::{SourceKey, SourceReads},
        template::runner::TemplateRunner,
    };
    use serde_json::json;
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    const STATION: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}" } },
    }"#;

    #[test]
    fn a_coalesced_batch_merges_each_record_under_its_own_mapping_s_read_set() {
        let mut runner = TemplateRunner::new();
        let mut reading = Mapping::from_json5(STATION, Path::new("test.json5"), &mut runner).unwrap();
        reading.set_source_reads(SourceReads::Keys([SourceKey::new("temperature")].into_iter().collect()));
        let reading = Arc::new(reading);
        let resolver = FragmentResolver::new(
            Box::new(RedbLatestEntityStore::new().unwrap()),
            Box::new(RedbRelationshipStore::new().unwrap()),
            runner.resolver(),
        );
        let quiet: Urn = "urn:ngsi-ld:Station:quiet".parse().unwrap();
        let loud: Urn = "urn:ngsi-ld:Station:loud".parse().unwrap();
        let batch = [
            (&quiet, json!({"level": 2, "temperature": 20})),
            (&quiet, json!({"level": 10, "temperature": 20})),
            (&loud, json!({"level": 2, "temperature": 20})),
            (&loud, json!({"level": 2, "temperature": 21})),
        ]
        .into_iter()
        .map(|(id, record)| Mapped::new(Fragment::new(record, id.clone(), None, None), Arc::clone(&reading)))
        .collect();

        let (results, _timing) = resolver.resolve_batch_timed(batch);

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.assemble(&quiet).unwrap().merge, None);
        let merge = resolver.assemble(&loud).unwrap().merge;
        assert_eq!(merge.map(|merge| merge.field.to_string()), Some("temperature".to_string()));
    }
}

//! The coalesced write path of [`FragmentResolver`]: a whole batch is prepared first, then flushed to
//! the relationship store and the entity store once each, which is what a transactional,
//! disk-backed store needs to stay at one round-trip per batch.

use crate::{
    entity_store::{error::EntityStoreError, record_timestamp::RecordTimestamp, store::FragmentWrite, write_batch::WriteBatch},
    fragment_resolver::FragmentResolver,
    mapping_id::MappingId,
    prepared_fragment::PreparedFragment,
    relationship_store::error::RelationshipStoreError,
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, source_position::SourcePosition};
use cassiopeia_mapping::mapping::Mapping;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use urn_rs::Urn;

/// The owned half of one entity-store write, set apart from the mapping whose read set the write
/// borrows.
struct OwnedWrite {
    base_id: Urn,
    source_data: Value,
    position: SourcePosition,
    mapping_id: MappingId,
    temporal: bool,
    timestamp: RecordTimestamp,
    scope: Option<NgsiLdScope>,
    expires_at: Option<DateTime<Utc>>,
}

impl FragmentResolver {
    /// Prepares each fragment's side-effects, interning its mapping, and returns the prepared
    /// fragments in the order of the batch with the time preparing them took.
    pub(crate) fn prepare_fragments(&self, fragments: Vec<Mapped<Fragment>>) -> (Vec<PreparedFragment>, Duration) {
        let prepare_started = Instant::now();
        let interned = self.intern_distinct(fragments.iter().map(|fragment| (fragment.mapping(), fragment.inner().variables())));
        let prepare = |fragment: Mapped<Fragment>| {
            let mapping_id = self.interned_id(&interned, fragment.mapping(), fragment.inner().variables());
            self.prepare(fragment, mapping_id)
        };

        let prepared: Vec<PreparedFragment> = match self.parallelism {
            Parallelism::Parallel => fragments.into_par_iter().map(prepare).collect(),
            Parallelism::Sequential => fragments.into_iter().map(prepare).collect(),
        };
        (prepared, prepare_started.elapsed())
    }

    /// Writes every prepared relationship edge in one batch, returning any failure message and the
    /// time the write took.
    pub(crate) fn write_relationship_edges(&self, prepared: &[PreparedFragment]) -> (Option<RelationshipStoreError>, Duration) {
        let edge_refs: Vec<(Urn, &str, Urn)> = prepared
            .iter()
            .flat_map(|prepared| {
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

    /// Writes every prepared fragment to the entity store as one batch, `batch`, in the order of the
    /// batch, and returns any failure message and the time the write took.
    ///
    /// Consuming the prepared fragments is what lets each record move into the store instead of being
    /// copied there. Each write borrows its mapping's read set, so the mappings are set aside first
    /// and kept alive for the whole store call. The `observedAt` failures are kept in the order of
    /// the batch, so the failure reported for a mapping is the first one in the source.
    pub(crate) fn write_entity_fragments(&self, prepared: Vec<PreparedFragment>, batch: WriteBatch) -> (Option<EntityStoreError>, Duration) {
        let (owned, mappings): (Vec<OwnedWrite>, Vec<Arc<Mapping>>) = prepared
            .into_iter()
            .map(|prepared| {
                let PreparedFragment {
                    base_id,
                    source_data,
                    position,
                    scope,
                    expires_at,
                    mapping,
                    mapping_id,
                    observed_at,
                    temporal,
                    ..
                } = prepared;
                (
                    OwnedWrite {
                        base_id,
                        source_data,
                        position,
                        mapping_id,
                        temporal,
                        timestamp: self.record_timestamp(mapping_id, observed_at),
                        scope,
                        expires_at,
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
                position: write.position,
                batch,
                mapping_id: write.mapping_id,
                temporal: write.temporal,
                timestamp: write.timestamp,
                scope: write.scope,
                expires_at: write.expires_at,
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
        test_order::next_position,
    };
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
    use cassiopeia_mapping::{
        mapping::Mapping,
        source_reads::{SourceKey, SourceReads},
        template::runner::TemplateRunner,
    };
    use pretty_assertions::assert_eq;
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
        .map(|(id, record)| {
            Mapped::new(
                Fragment::builder().source_data(record).target_urn(id.clone()).position(next_position()).build(),
                Arc::clone(&reading),
            )
        })
        .collect();

        let (results, _timing) = resolver.resolve_batch_timed(batch);

        assert!(results.iter().all(Result::is_ok));
        assert_eq!(resolver.assemble(&quiet).unwrap().merge, None);
        let merge = resolver.assemble(&loud).unwrap().merge;
        assert_eq!(merge.map(|merge| merge.field.to_string()), Some("temperature".to_string()));
    }
}

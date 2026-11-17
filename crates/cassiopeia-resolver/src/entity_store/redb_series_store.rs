use crate::{
    entity_store::{
        entity_order::in_source_order,
        error::{EntityStoreError, Result},
        mapping_expiry::{MappingExpiry, join_observations},
        mapping_reads::MappingReads,
        redb_database::RedbDatabase,
        redb_key::{base_id_bytes, distinct_id_count, make_prefix, prefix_end},
        redb_scope::{PendingScopes, SCOPES_TABLE, read_scope},
        store::{AssembledFragment, AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
        stored_observation::StoredObservation,
    },
    mapping_id::MappingId,
    store_action::StoreAction,
    store_write_strategy::StoreWriteStrategy,
};
use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
use redb::{Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use smallvec::smallvec;
use std::num::NonZeroU64;
use urn_rs::Urn;

const DATA_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("data");

/// Persistent disk-backed series entity store using [redb](https://docs.rs/redb).
///
/// Every stored fragment lives under the key `{base_id}\x1F{position_be}`: the big-endian position of
/// the fragment in the source, which no other fragment of the run shares. A batch is a sequence of
/// append-only inserts with no read and no reserialize. Assembly prefix-scans a base id's keys, which
/// sort in source order, into one emit-unit per observation, which the fold stage later folds into
/// one `EntityTemporal` (ETSI GS CIM 009 v1.9.1 clause 5.2.20) keeping an attribute without
/// `observedAt` from the first record in the source, and joins the expiries its observations carry
/// into the entity's. The ids are handed on in the order of their first records. Durability is
/// `Durability::None`: the database lives in a temporary directory cleaned up at the end of the run.
#[derive(Debug, Clone)]
pub struct RedbSeriesEntityStore {
    db: RedbDatabase,
}

impl RedbSeriesEntityStore {
    /// Creates a new redb-backed series entity store in a temporary directory.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the temporary
    /// directory or the database file cannot be created.
    pub fn new() -> Result<RedbSeriesEntityStore> {
        let db = RedbDatabase::open("entities.redb")?;
        db.init_tables(&[DATA_TABLE, SCOPES_TABLE])?;
        Ok(RedbSeriesEntityStore { db })
    }

    fn make_key(base_id: &Urn, position: SourcePosition) -> Vec<u8> {
        let mut key = make_prefix(base_id);
        key.extend_from_slice(&position.to_be_bytes());
        key
    }
}

impl EntityStore for RedbSeriesEntityStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::TransactionalBatch
    }

    fn store_fragment(&self, write: FragmentWrite<'_>) -> Result<()> {
        self.store_fragment_batch(vec![write])
    }

    fn store_fragment_batch(&self, fragments: Vec<FragmentWrite<'_>>) -> Result<()> {
        if fragments.is_empty() {
            return Ok(());
        }

        let mut pending_scopes = PendingScopes::default();

        let mut write_txn = self
            .db
            .database()
            .begin_write()
            .map_err(|e| self.db.err(e.into(), StoreAction::BeginTransaction))?;
        write_txn
            .set_durability(Durability::None)
            .map_err(|e| self.db.err(e.into(), StoreAction::SetDurability))?;

        {
            let mut data_table = write_txn.open_table(DATA_TABLE).map_err(|e| self.db.err(e.into(), StoreAction::OpenTable))?;
            let mut scope_table = write_txn.open_table(SCOPES_TABLE).map_err(|e| self.db.err(e.into(), StoreAction::OpenTable))?;

            for write in fragments {
                let FragmentWrite {
                    base_id,
                    source_data,
                    position,
                    mapping_id,
                    scope,
                    expires_at,
                    ..
                } = write;
                // Append-only: the fragment's own position gives a new key, so no read of prior state.
                let key = Self::make_key(&base_id, position);
                let stored = StoredObservation {
                    mapping_id,
                    data: source_data,
                    expires_at: MappingExpiry::first(expires_at),
                };
                let bytes = rmp_serde::to_vec(&stored)?;
                let replaced = data_table
                    .insert(key.as_slice(), bytes.as_slice())
                    .map_err(|e| self.db.err(e.into(), StoreAction::Insert))?
                    .is_some();
                // A position stands for one fragment, so a second write at it would discard an
                // observation rather than keep it.
                if replaced {
                    return Err(EntityStoreError::RepeatedPosition { entity: base_id });
                }

                if let Some(scope) = scope {
                    pending_scopes.add(&base_id, scope);
                }
            }

            pending_scopes.flush(&mut scope_table, &self.db)?;
        }

        write_txn.commit().map_err(|e| self.db.err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn get_entity_ids(&self) -> Result<Vec<Urn>> {
        // The keys of one id sort by position, so the first key of each id holds the position of its
        // first record: one scan over the keys gathers every id with it, and a sort puts them in that
        // order. Listing the ids takes this scan whatever their order, so ordering them costs the sort
        // alone.
        let read_txn = self
            .db
            .database()
            .begin_read()
            .map_err(|e| self.db.err(e.into(), StoreAction::BeginTransaction))?;
        let table = read_txn.open_table(DATA_TABLE).map_err(|e| self.db.err(e.into(), StoreAction::OpenTable))?;
        let mut firsts: Vec<(SourcePosition, Urn)> = Vec::new();
        let mut previous: Option<Vec<u8>> = None;
        for entry in table.iter().map_err(|e| self.db.err(e.into(), StoreAction::Iterate))? {
            let (key, _value) = entry.map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?;
            let key = key.value();
            let base = base_id_bytes(key);
            if previous.as_deref() == Some(base) {
                continue;
            }
            previous = Some(base.to_vec());
            let position = key
                .get(base.len() + 1..base.len() + 1 + SOURCE_POSITION_BYTES)
                .and_then(|bytes| <[u8; SOURCE_POSITION_BYTES]>::try_from(bytes).ok())
                .map(SourcePosition::from_be_bytes);
            if let (Some(position), Ok(urn)) = (position, String::from_utf8_lossy(base).parse::<Urn>()) {
                firsts.push((position, urn));
            }
        }
        Ok(in_source_order(firsts))
    }

    fn assemble_entity(&self, base_id: &Urn, _mappings: &dyn MappingReads) -> Result<AssembledFragments> {
        let prefix = make_prefix(base_id);
        let end = prefix_end(&prefix);

        let read_txn = self
            .db
            .database()
            .begin_read()
            .map_err(|e| self.db.err(e.into(), StoreAction::BeginTransaction))?;
        let data_table = read_txn.open_table(DATA_TABLE).map_err(|e| self.db.err(e.into(), StoreAction::OpenTable))?;
        let scope_table = read_txn.open_table(SCOPES_TABLE).map_err(|e| self.db.err(e.into(), StoreAction::OpenTable))?;

        let mut units: Vec<StoredUnit> = Vec::new();
        let mut expiries: Vec<(MappingId, MappingExpiry)> = Vec::new();
        for entry in data_table
            .range(prefix.as_slice()..end.as_slice())
            .map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?
        {
            let (_key, value) = entry.map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?;
            let stored: StoredObservation = rmp_serde::from_slice(value.value())?;
            expiries.push((stored.mapping_id, stored.expires_at));
            units.push(smallvec![AssembledFragment {
                mapping_id: stored.mapping_id,
                data: stored.data,
                // Every observation is kept on its own, so each stands for exactly one record and
                // has nothing to disagree with.
                records: NonZeroU64::MIN,
                conflict: None,
                unresolvable_discards: 0,
            }]);
        }

        let scope = read_scope(&scope_table, base_id, &self.db)?;
        let expires_at = join_observations(expiries);

        Ok(AssembledFragments { scope, expires_at, units })
    }

    fn count(&self) -> usize {
        distinct_id_count(&self.db, DATA_TABLE)
    }

    fn unit_count(&self) -> usize {
        let Ok(read_txn) = self.db.database().begin_read() else {
            return 0;
        };
        let Ok(table) = read_txn.open_table(DATA_TABLE) else {
            return 0;
        };
        usize::try_from(table.len().unwrap_or(0)).unwrap_or(usize::MAX)
    }

    fn clear(&self) -> Result<()> {
        let mut write_txn = self
            .db
            .database()
            .begin_write()
            .map_err(|e| self.db.err(e.into(), StoreAction::BeginTransaction))?;
        write_txn
            .set_durability(Durability::None)
            .map_err(|e| self.db.err(e.into(), StoreAction::SetDurability))?;
        for table in [DATA_TABLE, SCOPES_TABLE] {
            let _ = write_txn.delete_table(table).map_err(|e| self.db.err(e.into(), StoreAction::DeleteTable))?;
            write_txn.open_table(table).map_err(|e| self.db.err(e.into(), StoreAction::CreateTable))?;
        }
        write_txn.commit().map_err(|e| self.db.err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn destroy(&self) -> Result<()> {
        self.db.remove_files();
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn EntityStore> {
        Box::new(self.clone())
    }
}

impl Drop for RedbSeriesEntityStore {
    fn drop(&mut self) {
        self.db.remove_files();
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            error::EntityStoreError,
            record_timestamp::RecordTimestamp,
            redb_series_store::RedbSeriesEntityStore,
            store::{
                EntityStore,
                FragmentWrite,
                tests::{
                    READS_EVERYTHING,
                    assert_joins_expiries_across_mappings,
                    assert_merges_scopes_per_id,
                    assert_series_hands_ids_on_in_order_of_their_first_records,
                    assert_series_hands_observations_on_in_source_order,
                    assert_series_keeps_the_furthest_expiry_per_mapping,
                },
            },
        },
        mapping_id::MappingId,
        test_order::{ReadsEverything, next_batch, next_position},
    };
    use cassiopeia_mapping::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    #[test]
    fn scopes_from_every_fragment_of_an_id_are_merged() {
        assert_merges_scopes_per_id(&RedbSeriesEntityStore::new().unwrap());
    }

    #[test]
    fn the_expiries_of_an_id_s_mappings_are_joined() {
        assert_joins_expiries_across_mappings(&RedbSeriesEntityStore::new().unwrap());
    }

    #[test]
    fn an_id_s_observations_are_handed_on_in_source_order() {
        assert_series_hands_observations_on_in_source_order(&RedbSeriesEntityStore::new().unwrap());
    }

    #[test]
    fn ids_are_handed_on_in_the_order_of_their_first_records() {
        assert_series_hands_ids_on_in_order_of_their_first_records(&RedbSeriesEntityStore::new().unwrap());
    }

    #[test]
    fn a_mapping_s_observations_keep_their_furthest_expiry() {
        assert_series_keeps_the_furthest_expiry_per_mapping(&RedbSeriesEntityStore::new().unwrap());
    }

    fn write(base_id: &Urn, data: serde_json::Value, observed_at: &str) -> FragmentWrite<'static> {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            position: next_position(),
            batch: next_batch(),
            mapping_id: MappingId::new(1),
            temporal: true,
            timestamp: RecordTimestamp::Declared(ObservedAt::new(observed_at)),
            scope: None,
            expires_at: None,
            reads: &READS_EVERYTHING,
        }
    }

    #[test]
    fn three_observations_written_in_source_order_become_three_units_in_that_order() {
        let store = RedbSeriesEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");
        let first = json!({"on": true});
        let second = json!({"on": false});
        let third = json!({"on": true});

        store.store_fragment(write(&id, first.clone(), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, second.clone(), "2026-04-03T22:05:20Z")).unwrap();
        store.store_fragment(write(&id, third.clone(), "2026-04-03T22:10:20Z")).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units.len(), 3);
        assert_eq!(assembled.units[0][0].data, first);
        assert_eq!(assembled.units[1][0].data, second);
        assert_eq!(assembled.units[2][0].data, third);
    }

    #[test]
    fn every_stored_observation_stands_for_exactly_one_record_and_has_nothing_to_disagree_with() {
        let store = RedbSeriesEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"on": true}), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, json!({"on": false}), "2026-04-03T22:05:20Z")).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert!(
            assembled
                .units
                .iter()
                .flatten()
                .all(|fragment| fragment.records.get() == 1 && fragment.conflict.is_none())
        );
    }

    #[test]
    fn a_second_fragment_written_at_a_taken_position_is_refused_rather_than_kept_over_the_first() {
        let store = RedbSeriesEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");
        let first = write(&id, json!({"on": true}), "2026-04-03T22:00:20Z");
        let position = first.position;
        store.store_fragment(first).unwrap();

        let repeated = FragmentWrite {
            position,
            ..write(&id, json!({"on": false}), "2026-04-03T22:05:20Z")
        };
        let error = store.store_fragment(repeated).unwrap_err();

        assert!(matches!(error, EntityStoreError::RepeatedPosition { entity } if entity == id));
    }

    #[test]
    fn counts_report_ids_and_total_observations_and_ids_dedup() {
        let store = RedbSeriesEntityStore::new().unwrap();
        let one = urn("urn:ngsi-ld:Camera:1");
        let ten = urn("urn:ngsi-ld:Camera:10");

        store.store_fragment(write(&one, json!({"on": true}), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&one, json!({"on": false}), "2026-04-03T22:05:20Z")).unwrap();
        store.store_fragment(write(&ten, json!({"on": true}), "2026-04-03T22:00:20Z")).unwrap();

        assert_eq!(store.count(), 2);
        assert_eq!(store.unit_count(), 3);
        // A base id whose text is a prefix of another's does not merge into it.
        assert_eq!(store.get_entity_ids().unwrap(), vec![one, ten]);
    }
}

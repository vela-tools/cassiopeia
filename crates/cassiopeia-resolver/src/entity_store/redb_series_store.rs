use crate::{
    entity_store::{
        error::Result,
        redb_database::RedbDatabase,
        redb_key::{collect_ids, for_each_id_chunk, make_prefix, prefix_end},
        store::{AssembledFragment, AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
        stored_fragment::StoredFragment,
    },
    store_action::StoreAction,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::AHashMap;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use redb::{Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use smallvec::smallvec;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use urn_rs::Urn;

const DATA_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("data");
const SCOPES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("scopes");

/// Persistent disk-backed series entity store using [redb](https://docs.rs/redb).
///
/// Every stored fragment lives under the key `{base_id}\x1F{seq_be}` (an 8-byte big-endian insertion
/// sequence shared across clones), so a batch is a sequence of append-only inserts with no read and no
/// reserialize, mirroring the relationship store's sequenced composite key. Assembly prefix-scans a
/// base id's keys in insertion order into one emit-unit per observation, which the fold stage later
/// folds into one `EntityTemporal` (ETSI GS CIM 009 v1.9.1 clause 5.2.20). Durability is
/// `Durability::None`: the database lives in a temporary directory cleaned up at the end of the run.
#[derive(Debug, Clone)]
pub struct RedbSeriesEntityStore {
    db: RedbDatabase,
    /// Monotonic fragment counter shared across clones, giving each observation a distinct key.
    next_seq: Arc<AtomicU64>,
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
        Ok(RedbSeriesEntityStore {
            db,
            next_seq: Arc::new(AtomicU64::new(0)),
        })
    }

    fn make_key(base_id: &Urn, seq: u64) -> Vec<u8> {
        let mut key = make_prefix(base_id);
        key.extend_from_slice(&seq.to_be_bytes());
        key
    }
}

impl EntityStore for RedbSeriesEntityStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::TransactionalBatch
    }

    fn store_fragment(&self, write: FragmentWrite) -> Result<()> {
        self.store_fragment_batch(vec![write])
    }

    fn store_fragment_batch(&self, fragments: Vec<FragmentWrite>) -> Result<()> {
        if fragments.is_empty() {
            return Ok(());
        }

        let mut scope_writes: AHashMap<Vec<u8>, NgsiLdScope> = AHashMap::new();

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
                    mapping_id,
                    observed_at,
                    scope,
                    ..
                } = write;
                // Append-only: a fresh sequence number gives a new key, so no read of prior state.
                let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
                let key = Self::make_key(&base_id, seq);
                let stored = StoredFragment {
                    mapping_id,
                    data: source_data,
                    observed_at: observed_at.map(String::from),
                };
                let bytes = rmp_serde::to_vec(&stored)?;
                data_table
                    .insert(key.as_slice(), bytes.as_slice())
                    .map_err(|e| self.db.err(e.into(), StoreAction::Insert))?;

                if let Some(scope) = scope {
                    scope_writes.insert(base_id.as_str().as_bytes().to_vec(), scope);
                }
            }

            for (key, scope) in scope_writes {
                let encoded = rmp_serde::to_vec(&scope)?;
                let existing_matches = match scope_table.get(key.as_slice()).map_err(|e| self.db.err(e.into(), StoreAction::Read))? {
                    Some(guard) => guard.value() == encoded.as_slice(),
                    None => false,
                };
                if !existing_matches {
                    scope_table
                        .insert(key.as_slice(), encoded.as_slice())
                        .map_err(|e| self.db.err(e.into(), StoreAction::Insert))?;
                }
            }
        }

        write_txn.commit().map_err(|e| self.db.err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn get_entity_ids(&self) -> Result<Vec<Urn>> {
        collect_ids(&self.db, DATA_TABLE)
    }

    fn for_each_entity_id_chunk(&self, chunk_size: usize, callback: &mut dyn FnMut(Vec<Urn>) -> Result<()>) -> Result<()> {
        for_each_id_chunk(&self.db, DATA_TABLE, chunk_size, callback)
    }

    fn assemble_entity(&self, base_id: &Urn) -> Result<AssembledFragments> {
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
        for entry in data_table
            .range(prefix.as_slice()..end.as_slice())
            .map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?
        {
            let (_key, value) = entry.map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?;
            let stored: StoredFragment = rmp_serde::from_slice(value.value())?;
            units.push(smallvec![AssembledFragment {
                mapping_id: stored.mapping_id,
                data: stored.data,
            }]);
        }

        let scope = match scope_table
            .get(base_id.as_str().as_bytes())
            .map_err(|e| self.db.err(e.into(), StoreAction::Read))?
        {
            Some(guard) => Some(rmp_serde::from_slice(guard.value())?),
            None => None,
        };

        Ok(AssembledFragments { scope, units })
    }

    fn count(&self) -> usize {
        collect_ids(&self.db, DATA_TABLE).map_or(0, |ids| ids.len())
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
        {
            let _ = write_txn
                .delete_table(DATA_TABLE)
                .map_err(|e| self.db.err(e.into(), StoreAction::DeleteTable))?;
            let _ = write_txn
                .delete_table(SCOPES_TABLE)
                .map_err(|e| self.db.err(e.into(), StoreAction::DeleteTable))?;
            write_txn.open_table(DATA_TABLE).map_err(|e| self.db.err(e.into(), StoreAction::CreateTable))?;
            write_txn
                .open_table(SCOPES_TABLE)
                .map_err(|e| self.db.err(e.into(), StoreAction::CreateTable))?;
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
            redb_series_store::RedbSeriesEntityStore,
            store::{EntityStore, FragmentWrite},
        },
        mapping_id::MappingId,
    };
    use cassiopeia_mapping::observed_at::ObservedAt;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn write(base_id: &Urn, data: serde_json::Value, observed_at: &str) -> FragmentWrite {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            mapping_id: MappingId::new(1),
            temporal: true,
            observed_at: Some(ObservedAt::new(observed_at)),
            scope: None,
        }
    }

    #[test]
    fn three_observations_become_three_units_in_insertion_order() {
        let store = RedbSeriesEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");
        let first = json!({"on": true});
        let second = json!({"on": false});
        let third = json!({"on": true});

        store.store_fragment(write(&id, first.clone(), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, second.clone(), "2026-04-03T22:05:20Z")).unwrap();
        store.store_fragment(write(&id, third.clone(), "2026-04-03T22:10:20Z")).unwrap();

        let assembled = store.assemble_entity(&id).unwrap();
        assert_eq!(assembled.units.len(), 3);
        assert_eq!(assembled.units[0][0].data, first);
        assert_eq!(assembled.units[1][0].data, second);
        assert_eq!(assembled.units[2][0].data, third);
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

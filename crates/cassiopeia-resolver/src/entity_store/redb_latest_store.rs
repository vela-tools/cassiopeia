use crate::{
    entity_store::{
        error::Result,
        merge::deep_merge,
        redb_database::RedbDatabase,
        redb_key::{collect_ids, distinct_id_count, for_each_id_chunk, make_prefix, prefix_end},
        store::{AssembledFragment, AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
        stored_fragment::StoredFragment,
        supersession::supersedes,
    },
    mapping_id::MappingId,
    store_action::StoreAction,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::AHashMap;
use cassiopeia_mapping::observed_at::ObservedAt;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use redb::{Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde_json::Value;
use smallvec::SmallVec;
use std::collections::hash_map::Entry;
use urn_rs::Urn;

const DATA_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("data");
const SCOPES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("scopes");

/// Persistent disk-backed current-state entity store using [redb](https://docs.rs/redb).
///
/// Each mapping's retained fragment for a base id lives under the key `{base_id}\x1F{mapping_id_be}`.
/// A batch reads each key once, applies the current-state rule (a temporal mapping keeps the
/// greatest-`observedAt` record, a static mapping deep-merges), and commits once. Assembly prefix-scans
/// a base id's keys into one joined emit-unit. Durability is `Durability::None`: the database lives in
/// a temporary directory cleaned up at the end of the run, so fsync would be pure waste.
#[derive(Debug, Clone)]
pub struct RedbLatestEntityStore {
    db: RedbDatabase,
}

impl RedbLatestEntityStore {
    /// Creates a new redb-backed current-state entity store in a temporary directory.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the temporary
    /// directory or the database file cannot be created.
    pub fn new() -> Result<RedbLatestEntityStore> {
        let db = RedbDatabase::open("entities.redb")?;
        db.init_tables(&[DATA_TABLE, SCOPES_TABLE])?;
        Ok(RedbLatestEntityStore { db })
    }

    fn make_key(base_id: &Urn, mapping_id: MappingId) -> Vec<u8> {
        let mut key = make_prefix(base_id);
        key.extend_from_slice(&mapping_id.as_u32().to_be_bytes());
        key
    }
}

/// Coalesced in-memory state for one `(base id, mapping)` key within a batch.
struct LatestEntry {
    mapping_id: MappingId,
    data: Option<Value>,
    observed_at: Option<String>,
    scope: Option<NgsiLdScope>,
    base_key: Vec<u8>,
}

impl EntityStore for RedbLatestEntityStore {
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

        let mut grouped: AHashMap<Vec<u8>, LatestEntry> = AHashMap::new();

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
                    temporal,
                    observed_at,
                    scope,
                } = write;
                let key = Self::make_key(&base_id, mapping_id);
                let entry = match grouped.entry(key) {
                    Entry::Occupied(occupied) => occupied.into_mut(),
                    Entry::Vacant(vacant) => {
                        // Seed the accumulator from disk once per key.
                        let existing = data_table.get(vacant.key().as_slice()).map_err(|e| self.db.err(e.into(), StoreAction::Read))?;
                        let (data, observed_at) = match existing {
                            Some(guard) => {
                                let stored: StoredFragment = rmp_serde::from_slice(guard.value())?;
                                (Some(stored.data), stored.observed_at)
                            }
                            None => (None, None),
                        };
                        vacant.insert(LatestEntry {
                            mapping_id,
                            data,
                            observed_at,
                            scope: None,
                            base_key: base_id.as_str().as_bytes().to_vec(),
                        })
                    }
                };

                apply_write(entry, source_data, temporal, observed_at);
                if scope.is_some() {
                    entry.scope = scope;
                }
            }

            for (key, entry) in grouped {
                if let Some(data) = entry.data {
                    let stored = StoredFragment {
                        mapping_id: entry.mapping_id,
                        data,
                        observed_at: entry.observed_at,
                    };
                    let bytes = rmp_serde::to_vec(&stored)?;
                    data_table
                        .insert(key.as_slice(), bytes.as_slice())
                        .map_err(|e| self.db.err(e.into(), StoreAction::Insert))?;
                }

                if let Some(scope) = entry.scope {
                    let encoded = rmp_serde::to_vec(&scope)?;
                    let existing_matches = match scope_table
                        .get(entry.base_key.as_slice())
                        .map_err(|e| self.db.err(e.into(), StoreAction::Read))?
                    {
                        Some(guard) => guard.value() == encoded.as_slice(),
                        None => false,
                    };
                    if !existing_matches {
                        scope_table
                            .insert(entry.base_key.as_slice(), encoded.as_slice())
                            .map_err(|e| self.db.err(e.into(), StoreAction::Insert))?;
                    }
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

        let mut unit: StoredUnit = SmallVec::new();
        for entry in data_table
            .range(prefix.as_slice()..end.as_slice())
            .map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?
        {
            let (_key, value) = entry.map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?;
            let stored: StoredFragment = rmp_serde::from_slice(value.value())?;
            unit.push(AssembledFragment {
                mapping_id: stored.mapping_id,
                data: stored.data,
            });
        }

        // The prefix scan already returns each mapping in big-endian id order, so the joined unit is
        // deterministic without an extra sort.
        let units = if unit.is_empty() { Vec::new() } else { vec![unit] };
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
        distinct_id_count(&self.db, DATA_TABLE)
    }

    fn unit_count(&self) -> usize {
        // Current-state assembly emits one unit per id.
        distinct_id_count(&self.db, DATA_TABLE)
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

impl Drop for RedbLatestEntityStore {
    fn drop(&mut self) {
        self.db.remove_files();
    }
}

/// Applies one write to its coalesced accumulator under the current-state rule.
fn apply_write(entry: &mut LatestEntry, source_data: Value, temporal: bool, observed_at: Option<ObservedAt>) {
    if temporal {
        // A temporal mapping's records are observations of one id: keep the latest.
        let new = observed_at.as_ref().map(ObservedAt::as_str);
        if entry.data.is_none() || supersedes(new, entry.observed_at.as_deref()) {
            entry.data = Some(source_data);
            entry.observed_at = observed_at.map(String::from);
        }
    } else {
        // A static mapping's records refine one entity: merge them.
        match &mut entry.data {
            Some(accumulator) => deep_merge(accumulator, &source_data),
            slot @ None => *slot = Some(source_data),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            redb_latest_store::RedbLatestEntityStore,
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

    fn write(base_id: &Urn, data: serde_json::Value, mapping: u32, temporal: bool, observed_at: Option<&str>) -> FragmentWrite {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            mapping_id: MappingId::new(mapping),
            temporal,
            observed_at: observed_at.map(ObservedAt::new),
            scope: None,
        }
    }

    #[test]
    fn two_mappings_for_one_id_join_into_one_unit() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");
        let geometry = json!({"location": {"type": "Point"}});
        let state = json!({"on": true});

        store
            .store_fragment_batch(vec![write(&id, geometry, 1, false, None), write(&id, state, 2, false, None)])
            .unwrap();

        let assembled = store.assemble_entity(&id).unwrap();
        assert_eq!(assembled.units.len(), 1);
        assert_eq!(assembled.units[0].len(), 2);
        assert_eq!(store.count(), 1);
        assert_eq!(store.get_entity_ids().unwrap(), vec![id]);
    }

    #[test]
    fn a_temporal_mapping_keeps_the_max_observed_at_record_across_batches() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");
        let early = json!({"on": false});
        let late = json!({"on": true});
        let middle = json!({"on": false});

        store.store_fragment(write(&id, early, 1, true, Some("2026-04-03T22:00:20Z"))).unwrap();
        store.store_fragment(write(&id, late.clone(), 1, true, Some("2026-04-03T22:10:20Z"))).unwrap();
        store.store_fragment(write(&id, middle, 1, true, Some("2026-04-03T22:05:20Z"))).unwrap();

        let assembled = store.assemble_entity(&id).unwrap();
        assert_eq!(assembled.units[0].len(), 1);
        assert_eq!(assembled.units[0][0].data, late);
    }

    #[test]
    fn a_static_mapping_deep_merges_across_batches() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"b": 2}), 1, false, None)).unwrap();

        let assembled = store.assemble_entity(&id).unwrap();
        assert_eq!(assembled.units[0][0].data, json!({"a": 1, "b": 2}));
    }
}

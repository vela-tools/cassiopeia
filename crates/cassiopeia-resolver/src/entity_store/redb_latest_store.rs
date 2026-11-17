use crate::{
    entity_store::{
        error::Result,
        latest_slot::{IncomingRecord, LatestSlot},
        mapping_expiry::join,
        mapping_reads::MappingReads,
        redb_database::RedbDatabase,
        redb_key::{collect_ids, distinct_id_count, for_each_id_chunk, make_prefix, prefix_end},
        redb_scope::{PendingScopes, SCOPES_TABLE, read_scope},
        store::{AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
        stored_slot::StoredSlot,
    },
    mapping_id::MappingId,
    store_action::StoreAction,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::AHashMap;
use cassiopeia_mapping::source_reads::SourceReads;
use redb::{Durability, ReadableDatabase, ReadableTable, TableDefinition};
use smallvec::SmallVec;
use std::collections::hash_map::Entry;
use urn_rs::Urn;

const DATA_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("data");

/// Persistent disk-backed current-state entity store using [redb](https://docs.rs/redb).
///
/// Each mapping's [`LatestSlot`] for a base id lives under the key `{base_id}\x1F{mapping_id_be}`,
/// encoded with `rmp-serde`. A batch reads each key once, folds the batch's records into the slot by
/// the current-state rule (a temporal mapping keeps the greatest-`observedAt` record, a static mapping
/// merges in source order), and commits once. The whole batch is in hand, so every slot it touched is
/// sealed before it is written back. Assembly prefix-scans a base id's keys into one joined emit-unit,
/// joining the expiry each key carries into the entity's. Durability is `Durability::None`: the
/// database lives in a temporary directory cleaned up at the end of the run, so fsync would be pure
/// waste.
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

/// One `(base id, mapping)` key's slot within a batch, with the read set of the mapping it seals with.
struct BatchSlot<'a> {
    /// The key's slot, seeded from disk; `None` until a record has landed on it.
    slot: Option<LatestSlot>,
    /// The read set of the mapping, from the batch's writes.
    reads: &'a SourceReads,
}

impl EntityStore for RedbLatestEntityStore {
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

        // A batch touches at most one key per write, so sizing the map for every write up front spares
        // it the rehashes that would move each slot several times as it grows.
        let mut grouped: AHashMap<Vec<u8>, BatchSlot<'_>> = AHashMap::with_capacity(fragments.len());
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
                    batch,
                    mapping_id,
                    temporal,
                    timestamp,
                    scope,
                    expires_at,
                    reads,
                } = write;
                let key = Self::make_key(&base_id, mapping_id);
                let entry = match grouped.entry(key) {
                    Entry::Occupied(occupied) => occupied.into_mut(),
                    Entry::Vacant(vacant) => {
                        // Seed the accumulator from disk once per key.
                        let existing = data_table.get(vacant.key().as_slice()).map_err(|e| self.db.err(e.into(), StoreAction::Read))?;
                        let slot = match existing {
                            Some(guard) => Some(LatestSlot::from_stored(rmp_serde::from_slice::<StoredSlot>(guard.value())?, batch)),
                            None => None,
                        };
                        vacant.insert(BatchSlot { slot, reads })
                    }
                };

                let incoming = IncomingRecord {
                    data: source_data,
                    position,
                    batch,
                    timestamp,
                    expires_at,
                };
                match &mut entry.slot {
                    Some(slot) => slot.absorb(incoming, reads),
                    None => entry.slot = Some(LatestSlot::new(incoming, temporal)),
                }
                if let Some(scope) = scope {
                    pending_scopes.add(&base_id, scope);
                }
            }

            for (key, BatchSlot { slot, reads }) in grouped {
                if let Some(slot) = slot {
                    // Every record of the batch is in, so the slot is written back with them merged.
                    let bytes = rmp_serde::to_vec(&slot.into_stored(reads))?;
                    data_table
                        .insert(key.as_slice(), bytes.as_slice())
                        .map_err(|e| self.db.err(e.into(), StoreAction::Insert))?;
                }
            }

            pending_scopes.flush(&mut scope_table, &self.db)?;
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

        let mut unit: StoredUnit = SmallVec::new();
        let mut expiries = SmallVec::<[_; 1]>::new();
        for entry in data_table
            .range(prefix.as_slice()..end.as_slice())
            .map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?
        {
            let (key, value) = entry.map_err(|e| self.db.err(e.into(), StoreAction::Iterate))?;
            // Every batch writes the slots it touched back with their records merged, so a stored slot
            // needs no read set to finish.
            let stored: StoredSlot = rmp_serde::from_slice(value.value())?;
            let (fragment, expiry) = stored.into_fragment(mapping_id_of(key.value()));
            expiries.push(expiry);
            unit.push(fragment);
        }

        // The prefix scan already returns each mapping in big-endian id order, so the joined unit is
        // deterministic without an extra sort.
        let units = if unit.is_empty() { Vec::new() } else { vec![unit] };
        let scope = read_scope(&scope_table, base_id, &self.db)?;
        let expires_at = join(expiries);

        Ok(AssembledFragments { scope, expires_at, units })
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

/// The mapping id a key ends in: the four big-endian bytes after the base id and its separator.
fn mapping_id_of(key: &[u8]) -> MappingId {
    let id = key
        .get(key.len().saturating_sub(4)..)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(0, u32::from_be_bytes);
    MappingId::new(id)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            record_timestamp::RecordTimestamp,
            redb_latest_store::RedbLatestEntityStore,
            store::{
                EntityStore,
                FragmentWrite,
                tests::{READS_EVERYTHING, assert_expiry_follows_the_retained_record, assert_joins_expiries_across_mappings, assert_merges_scopes_per_id},
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
        assert_merges_scopes_per_id(&RedbLatestEntityStore::new().unwrap());
    }

    #[test]
    fn the_expiries_of_an_id_s_mappings_are_joined() {
        assert_joins_expiries_across_mappings(&RedbLatestEntityStore::new().unwrap());
    }

    #[test]
    fn a_mapping_s_expiry_follows_the_record_it_retains() {
        assert_expiry_follows_the_retained_record(&RedbLatestEntityStore::new().unwrap());
    }

    fn write(base_id: &Urn, data: serde_json::Value, mapping: u32, temporal: bool, observed_at: Option<&str>) -> FragmentWrite<'static> {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            position: next_position(),
            batch: next_batch(),
            mapping_id: MappingId::new(mapping),
            temporal,
            timestamp: RecordTimestamp::from(observed_at.map(ObservedAt::new)),
            scope: None,
            expires_at: None,
            reads: &READS_EVERYTHING,
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

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units.len(), 1);
        assert_eq!(assembled.units[0].len(), 2);
        assert_eq!(store.count(), 1);
        assert_eq!(store.get_entity_ids().unwrap(), vec![id]);
    }

    #[test]
    fn a_temporal_mapping_keeps_the_max_observed_at_record_across_batches() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");
        // Only the superseded record carries `battery`, so a merge rather than a replacement would
        // leave it in the retained record.
        let early = json!({"on": false, "battery": 80});
        let late = json!({"on": true});
        let middle = json!({"on": false});

        store.store_fragment(write(&id, early, 1, true, Some("2026-04-03T22:00:20Z"))).unwrap();
        store.store_fragment(write(&id, late.clone(), 1, true, Some("2026-04-03T22:10:20Z"))).unwrap();
        store.store_fragment(write(&id, middle, 1, true, Some("2026-04-03T22:05:20Z"))).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units[0].len(), 1);
        assert_eq!(assembled.units[0][0].data, late);
    }

    #[test]
    fn each_mapping_slot_counts_every_record_across_and_within_batches() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store
            .store_fragment_batch(vec![
                write(&id, json!({"b": 2}), 1, false, None),
                write(&id, json!({"c": 3}), 1, false, None),
                write(&id, json!({"on": true}), 2, true, Some("2026-04-03T22:00:20Z")),
            ])
            .unwrap();
        store
            .store_fragment(write(&id, json!({"on": false}), 2, true, Some("2026-04-03T21:00:20Z")))
            .unwrap();

        let unit = &store.assemble_entity(&id, &ReadsEverything).unwrap().units[0];
        let counts: Vec<u64> = unit.iter().map(|fragment| fragment.records.get()).collect();
        assert_eq!(counts, [3, 2]);
        assert_eq!(unit[0].data, json!({"a": 1, "b": 2, "c": 3}));
        assert_eq!(unit[1].data, json!({"on": true}));
    }

    #[test]
    fn a_slot_remembers_the_earliest_field_its_records_disagreed_on_across_batches() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1, "z": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"z": 2}), 1, false, None)).unwrap();
        store
            .store_fragment_batch(vec![write(&id, json!({"b": 2}), 1, false, None), write(&id, json!({"a": 3}), 1, false, None)])
            .unwrap();
        store.store_fragment(write(&id, json!({"c": 1}), 2, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"c": 1}), 2, false, None)).unwrap();

        let unit = &store.assemble_entity(&id, &ReadsEverything).unwrap().units[0];
        let conflicts: Vec<Option<String>> = unit.iter().map(|fragment| fragment.conflict.as_ref().map(ToString::to_string)).collect();
        assert_eq!(conflicts, [Some("a".to_string()), None]);
        assert_eq!(unit[0].data, json!({"a": 1, "z": 1, "b": 2}));
    }

    #[test]
    fn a_static_mapping_deep_merges_across_batches() {
        let store = RedbLatestEntityStore::new().unwrap();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"b": 2}), 1, false, None)).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units[0][0].data, json!({"a": 1, "b": 2}));
    }
}

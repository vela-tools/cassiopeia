use crate::{
    relationship_store::{
        error::{RelationshipStoreError, Result},
        store::{RelationshipStore, StoredRelationships},
    },
    store_action::StoreAction,
    store_write_strategy::StoreWriteStrategy,
};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use std::{
    fs,
    path::PathBuf,
    str::from_utf8,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tracing::info;
use urn_rs::Urn;

/// Unit separator delimiting components in composite keys.
const KEY_SEPARATOR: u8 = 0x1F;

/// Target cache size (256 MiB). Matches the entity-store sizing rationale.
const CACHE_BYTES: usize = 256 * 1024 * 1024;

const RELATIONSHIPS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("relationships");

/// Persistent disk-backed relationship store using [redb](https://docs.rs/redb).
///
/// Keys are `{parent}\x1F{property}\x1F{seq_be}` (an 8-byte big-endian
/// insertion sequence number) mapping to the child URN bytes as the value.
/// Sequencing edges rather than keying on the child makes a prefix range scan
/// return each `(parent, property)` list in exact insertion order and retains
/// duplicate edges, matching [`DashMapRelationshipStore`](crate::relationship_store::dashmap_store::DashMapRelationshipStore).
/// [`add_child`](RelationshipStore::add_child) and
/// [`add_child_batch`](RelationshipStore::add_child_batch) commit a single
/// `WriteTransaction`; [`take_all_relationships`](RelationshipStore::take_all_relationships)
/// prefix-scans and removes in one write transaction (collecting keys first,
/// since the range iterator borrows the table and rules out in-iter removal).
#[derive(Debug)]
pub struct RedbRelationshipStore {
    db: Arc<Database>,
    temp_dir_path: PathBuf,
    db_path: PathBuf,
    /// Monotonic edge counter shared across clones. A duplicate `(parent,
    /// property, child)` edge gets a distinct key, so duplicates survive and
    /// insertion order is recoverable by byte-sorting the big-endian sequence.
    next_seq: Arc<AtomicU64>,
}

impl RedbRelationshipStore {
    /// Creates a new redb-backed relationship store in a temporary directory.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the temporary directory or the database file cannot be created.
    pub fn new() -> Result<RedbRelationshipStore> {
        let temp_dir = tempfile::tempdir().map_err(|source| RelationshipStoreError::CreateTempDirectory { source })?;
        let temp_dir_path = temp_dir.keep();
        let db_path = temp_dir_path.join("relationships.redb");

        let db = Database::builder()
            .set_cache_size(CACHE_BYTES)
            .create(&db_path)
            .map_err(|e| RelationshipStoreError::RedbOperation {
                source: Box::new(e.into()),
                path: db_path.clone(),
                action: StoreAction::OpenDatabase,
            })?;

        let write_txn = db.begin_write().map_err(|e| RelationshipStoreError::RedbOperation {
            source: Box::new(e.into()),
            path: db_path.clone(),
            action: StoreAction::BeginTransaction,
        })?;
        {
            write_txn.open_table(RELATIONSHIPS_TABLE).map_err(|e| RelationshipStoreError::RedbOperation {
                source: Box::new(e.into()),
                path: db_path.clone(),
                action: StoreAction::CreateTable,
            })?;
        }
        write_txn.commit().map_err(|e| RelationshipStoreError::RedbOperation {
            source: Box::new(e.into()),
            path: db_path.clone(),
            action: StoreAction::CommitTransaction,
        })?;

        info!("Relationship store created (redb)");

        Ok(RedbRelationshipStore {
            db: Arc::new(db),
            temp_dir_path,
            db_path,
            next_seq: Arc::new(AtomicU64::new(0)),
        })
    }

    fn make_key(parent_urn: &Urn, property: &str, seq: u64) -> Vec<u8> {
        let parent = parent_urn.as_str();
        let seq_be = seq.to_be_bytes();
        let mut key = Vec::with_capacity(parent.len() + 1 + property.len() + 1 + seq_be.len());
        key.extend_from_slice(parent.as_bytes());
        key.push(KEY_SEPARATOR);
        key.extend_from_slice(property.as_bytes());
        key.push(KEY_SEPARATOR);
        key.extend_from_slice(&seq_be);
        key
    }

    fn make_parent_prefix(parent_urn: &Urn) -> Vec<u8> {
        let parent = parent_urn.as_str();
        let mut prefix = Vec::with_capacity(parent.len() + 1);
        prefix.extend_from_slice(parent.as_bytes());
        prefix.push(KEY_SEPARATOR);
        prefix
    }

    /// Computes an exclusive upper bound for a prefix scan by incrementing the
    /// trailing byte. The separator is `0x1F`, so a carry is impossible.
    fn prefix_end(prefix: &[u8]) -> Vec<u8> {
        let mut end = prefix.to_vec();
        if let Some(last) = end.last_mut() {
            *last = last.saturating_add(1);
        }
        end
    }

    /// Extracts the property name from a key remainder of the form
    /// `{property}\x1F{seq_be}`. Only the bytes up to the first separator are
    /// UTF-8 decoded; the trailing 8-byte big-endian sequence is opaque and may
    /// itself contain `0x1F` bytes, so it is never scanned or parsed.
    fn parse_property(remainder: &[u8]) -> Result<String> {
        let sep_pos = remainder
            .iter()
            .position(|&byte| byte == KEY_SEPARATOR)
            .ok_or_else(|| RelationshipStoreError::InvalidKey {
                key: String::from_utf8_lossy(remainder).to_string(),
            })?;

        let property_bytes = &remainder[..sep_pos];
        let property = from_utf8(property_bytes).map_err(|_| RelationshipStoreError::InvalidKey {
            key: String::from_utf8_lossy(property_bytes).to_string(),
        })?;
        Ok(property.to_string())
    }

    /// Parses a child URN from the stored value bytes.
    fn parse_child(value_bytes: &[u8]) -> Result<Urn> {
        let child_str = from_utf8(value_bytes).map_err(|_| RelationshipStoreError::InvalidKey {
            key: String::from_utf8_lossy(value_bytes).to_string(),
        })?;
        child_str
            .parse::<Urn>()
            .map_err(|_| RelationshipStoreError::InvalidKey { key: child_str.to_string() })
    }

    fn redb_err(&self, source: redb::Error, action: StoreAction) -> RelationshipStoreError {
        RelationshipStoreError::RedbOperation {
            source: Box::new(source),
            path: self.db_path.clone(),
            action,
        }
    }
}

impl Clone for RedbRelationshipStore {
    fn clone(&self) -> RedbRelationshipStore {
        RedbRelationshipStore {
            db: Arc::clone(&self.db),
            temp_dir_path: self.temp_dir_path.clone(),
            db_path: self.db_path.clone(),
            next_seq: Arc::clone(&self.next_seq),
        }
    }
}

impl RelationshipStore for RedbRelationshipStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::TransactionalBatch
    }

    fn add_child(&self, parent_urn: &Urn, property: &str, child_urn: &Urn) -> Result<()> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let key = Self::make_key(parent_urn, property, seq);

        let mut write_txn = self.db.begin_write().map_err(|e| self.redb_err(e.into(), StoreAction::BeginTransaction))?;
        write_txn
            .set_durability(Durability::None)
            .map_err(|e| self.redb_err(e.into(), StoreAction::SetDurability))?;
        {
            let mut table = write_txn
                .open_table(RELATIONSHIPS_TABLE)
                .map_err(|e| self.redb_err(e.into(), StoreAction::OpenTable))?;
            table
                .insert(key.as_slice(), child_urn.as_str().as_bytes())
                .map_err(|e| self.redb_err(e.into(), StoreAction::Insert))?;
        }
        write_txn.commit().map_err(|e| self.redb_err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn add_child_batch(&self, entries: &[(Urn, &str, Urn)]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut write_txn = self.db.begin_write().map_err(|e| self.redb_err(e.into(), StoreAction::BeginTransaction))?;
        write_txn
            .set_durability(Durability::None)
            .map_err(|e| self.redb_err(e.into(), StoreAction::SetDurability))?;
        {
            let mut table = write_txn
                .open_table(RELATIONSHIPS_TABLE)
                .map_err(|e| self.redb_err(e.into(), StoreAction::OpenTable))?;
            for (parent, property, child) in entries {
                let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
                let key = Self::make_key(parent, property, seq);
                table
                    .insert(key.as_slice(), child.as_str().as_bytes())
                    .map_err(|e| self.redb_err(e.into(), StoreAction::Insert))?;
            }
        }
        write_txn.commit().map_err(|e| self.redb_err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn take_all_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        let prefix = Self::make_parent_prefix(parent_urn);
        let end = Self::prefix_end(&prefix);

        let mut result = StoredRelationships::default();
        let mut keys_to_remove: Vec<Vec<u8>> = Vec::new();

        let mut write_txn = self.db.begin_write().map_err(|e| self.redb_err(e.into(), StoreAction::BeginTransaction))?;
        write_txn
            .set_durability(Durability::None)
            .map_err(|e| self.redb_err(e.into(), StoreAction::SetDurability))?;
        {
            let mut table = write_txn
                .open_table(RELATIONSHIPS_TABLE)
                .map_err(|e| self.redb_err(e.into(), StoreAction::OpenTable))?;

            {
                let range = table
                    .range(prefix.as_slice()..end.as_slice())
                    .map_err(|e| self.redb_err(e.into(), StoreAction::Iterate))?;
                for entry in range {
                    let (key, value) = entry.map_err(|e| self.redb_err(e.into(), StoreAction::Iterate))?;
                    let key_bytes = key.value();
                    let remainder = &key_bytes[prefix.len()..];
                    let property = Self::parse_property(remainder)?;
                    let child_urn = Self::parse_child(value.value())?;
                    result.entry(property).or_default().push(child_urn);
                    keys_to_remove.push(key_bytes.to_vec());
                }
            }

            for key in &keys_to_remove {
                table.remove(key.as_slice()).map_err(|e| self.redb_err(e.into(), StoreAction::Remove))?;
            }
        }
        write_txn.commit().map_err(|e| self.redb_err(e.into(), StoreAction::CommitTransaction))?;

        Ok(result)
    }

    fn get_all_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        // Read-only scan: redb read transactions are MVCC and lock-free, so
        // `rayon::par_iter` callers can assemble entities concurrently instead of
        // serializing on the single writer.
        let prefix = Self::make_parent_prefix(parent_urn);
        let end = Self::prefix_end(&prefix);

        let read_txn = self.db.begin_read().map_err(|e| self.redb_err(e.into(), StoreAction::BeginTransaction))?;
        let table = read_txn
            .open_table(RELATIONSHIPS_TABLE)
            .map_err(|e| self.redb_err(e.into(), StoreAction::OpenTable))?;

        let mut result = StoredRelationships::default();
        let range = table
            .range(prefix.as_slice()..end.as_slice())
            .map_err(|e| self.redb_err(e.into(), StoreAction::Iterate))?;
        for entry in range {
            let (key, value) = entry.map_err(|e| self.redb_err(e.into(), StoreAction::Iterate))?;
            let key_bytes = key.value();
            let remainder = &key_bytes[prefix.len()..];
            let property = Self::parse_property(remainder)?;
            let child_urn = Self::parse_child(value.value())?;
            result.entry(property).or_default().push(child_urn);
        }
        Ok(result)
    }

    fn destroy(&self) -> Result<()> {
        let mut write_txn = self.db.begin_write().map_err(|e| self.redb_err(e.into(), StoreAction::BeginTransaction))?;
        write_txn
            .set_durability(Durability::None)
            .map_err(|e| self.redb_err(e.into(), StoreAction::SetDurability))?;
        {
            let _ = write_txn
                .delete_table(RELATIONSHIPS_TABLE)
                .map_err(|e| self.redb_err(e.into(), StoreAction::DeleteTable))?;
            write_txn
                .open_table(RELATIONSHIPS_TABLE)
                .map_err(|e| self.redb_err(e.into(), StoreAction::CreateTable))?;
        }
        write_txn.commit().map_err(|e| self.redb_err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn RelationshipStore> {
        Box::new(self.clone())
    }
}

impl Drop for RedbRelationshipStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir_path);
    }
}

#[cfg(test)]
mod tests {
    use crate::relationship_store::{
        redb_store::RedbRelationshipStore,
        store::{RelationshipStore, tests::assert_preserves_insertion_order_and_duplicates},
    };
    use urn_rs::Urn;

    #[test]
    fn honours_cross_backend_order_and_duplicate_contract() {
        let store = RedbRelationshipStore::new().unwrap();
        assert_preserves_insertion_order_and_duplicates(&store);
    }

    #[test]
    fn read_back_is_insertion_order_not_sorted() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child_c: Urn = "urn:ngsi-ld:Child:C".parse().unwrap();
        let child_a: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();
        let child_b: Urn = "urn:ngsi-ld:Child:B".parse().unwrap();

        store
            .add_child_batch(&[
                (parent.clone(), "hasPart", child_c.clone()),
                (parent.clone(), "hasPart", child_a.clone()),
                (parent.clone(), "hasPart", child_b.clone()),
            ])
            .unwrap();

        let rels = store.take_all_relationships(&parent).unwrap();
        assert_eq!(rels.get("hasPart").map(Vec::as_slice), Some(&[child_c, child_a, child_b][..]));
    }

    #[test]
    fn duplicate_edges_are_retained() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child: Urn = "urn:ngsi-ld:Child:X".parse().unwrap();

        store.add_child(&parent, "hasPart", &child).unwrap();
        store.add_child(&parent, "hasPart", &child).unwrap();

        let rels = store.take_all_relationships(&parent).unwrap();
        assert_eq!(rels.get("hasPart").map(Vec::as_slice), Some(&[child.clone(), child][..]));
    }

    #[test]
    fn each_property_keeps_its_own_insertion_order() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let first_b: Urn = "urn:ngsi-ld:First:B".parse().unwrap();
        let first_a: Urn = "urn:ngsi-ld:First:A".parse().unwrap();
        let second_b: Urn = "urn:ngsi-ld:Second:B".parse().unwrap();
        let second_a: Urn = "urn:ngsi-ld:Second:A".parse().unwrap();

        store
            .add_child_batch(&[
                (parent.clone(), "first", first_b.clone()),
                (parent.clone(), "second", second_b.clone()),
                (parent.clone(), "first", first_a.clone()),
                (parent.clone(), "second", second_a.clone()),
            ])
            .unwrap();

        let rels = store.take_all_relationships(&parent).unwrap();
        assert_eq!(rels.get("first").map(Vec::as_slice), Some(&[first_b, first_a][..]));
        assert_eq!(rels.get("second").map(Vec::as_slice), Some(&[second_b, second_a][..]));
    }

    #[test]
    fn add_and_take_roundtrip() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child_a: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();
        let child_b: Urn = "urn:ngsi-ld:Child:B".parse().unwrap();

        store
            .add_child_batch(&[(parent.clone(), "hasPart", child_a.clone()), (parent.clone(), "hasPart", child_b.clone())])
            .unwrap();

        let rels = store.take_all_relationships(&parent).unwrap();
        assert_eq!(rels.get("hasPart").map(Vec::as_slice), Some(&[child_a, child_b][..]));

        let rels_again = store.take_all_relationships(&parent).unwrap();
        assert!(rels_again.is_empty());
    }

    #[test]
    fn get_all_is_non_destructive() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();

        store.add_child(&parent, "hasPart", &child).unwrap();

        let first = store.get_all_relationships(&parent).unwrap();
        assert_eq!(first.get("hasPart").map(Vec::as_slice), Some(&[child.clone()][..]));

        let second = store.get_all_relationships(&parent).unwrap();
        assert_eq!(second.get("hasPart").map(Vec::as_slice), Some(&[child][..]));
    }

    #[test]
    fn prefix_scan_does_not_pick_up_other_parents() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent_a: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let parent_b: Urn = "urn:ngsi-ld:Parent:2".parse().unwrap();
        let child: Urn = "urn:ngsi-ld:Child:X".parse().unwrap();

        store.add_child(&parent_a, "has", &child).unwrap();
        store.add_child(&parent_b, "has", &child).unwrap();

        let rels_a = store.take_all_relationships(&parent_a).unwrap();
        assert_eq!(rels_a.get("has").map(Vec::as_slice), Some(&[child.clone()][..]));

        let rels_b = store.take_all_relationships(&parent_b).unwrap();
        assert_eq!(rels_b.get("has").map(Vec::as_slice), Some(&[child][..]));
    }
}

use crate::{
    relationship_store::{
        error::{RelationshipStoreError, Result},
        relationship_edge::FragmentEdges,
        store::{RelationshipStore, StoredRelationships, join},
    },
    store_action::StoreAction,
    store_write_strategy::StoreWriteStrategy,
};
use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, Table, TableDefinition};
use std::{fs, path::PathBuf, str::from_utf8, sync::Arc};
use tracing::info;
use urn_rs::Urn;

/// Unit separator delimiting components in composite keys and values.
const KEY_SEPARATOR: u8 = 0x1F;

/// The number of bytes a target's place among its fragment's targets takes in a key.
const ORDINAL_BYTES: usize = 4;

/// Target cache size (256 MiB). Matches the entity-store sizing rationale.
const CACHE_BYTES: usize = 256 * 1024 * 1024;

const RELATIONSHIPS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("relationships");

/// Persistent disk-backed relationship store using [redb](https://docs.rs/redb).
///
/// Keys are `{parent}\x1F{position_be}{ordinal_be}`: the parent, the 16-byte big-endian
/// [`SourcePosition`] of the fragment that gave the target, then the 4-byte big-endian place of the
/// target among that fragment's targets. Each maps to `{property}\x1F{child}`. Big-endian positions
/// sort bytewise as the positions do, so a prefix range scan of one parent returns its targets in the
/// order of the source without a sort, however the writes of a batch were ordered; a repeated edge
/// is stored again and collapses onto its earliest place when read, matching
/// [`DashMapRelationshipStore`](crate::relationship_store::dashmap_store::DashMapRelationshipStore).
/// No two fragments share a position and each records its edges in one call, so no two edges share
/// a key. [`add_edges`](RelationshipStore::add_edges) and
/// [`add_edge_batch`](RelationshipStore::add_edge_batch) commit a single
/// `WriteTransaction`; [`assemble_relationships`](RelationshipStore::assemble_relationships)
/// prefix-scans one parent in a read transaction and leaves its rows in place.
#[derive(Debug, Clone)]
pub struct RedbRelationshipStore {
    db: Arc<Database>,
    temp_dir_path: PathBuf,
    db_path: PathBuf,
}

impl RedbRelationshipStore {
    /// Creates a new redb-backed relationship store in a temporary directory.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`] when the temporary directory or the database file cannot be created.
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
        })
    }

    /// The key of the target at `ordinal` among those the fragment at `position` gave `parent_urn`.
    fn make_key(parent_urn: &Urn, position: SourcePosition, ordinal: u32) -> Vec<u8> {
        let parent = parent_urn.as_str();
        let mut key = Vec::with_capacity(parent.len() + 1 + SOURCE_POSITION_BYTES + ORDINAL_BYTES);
        key.extend_from_slice(parent.as_bytes());
        key.push(KEY_SEPARATOR);
        key.extend_from_slice(&position.to_be_bytes());
        key.extend_from_slice(&ordinal.to_be_bytes());
        key
    }

    /// The value of a target `child_urn` given under the relationship key `property`.
    fn make_value(property: &str, child_urn: &Urn) -> Vec<u8> {
        let child = child_urn.as_str();
        let mut value = Vec::with_capacity(property.len() + 1 + child.len());
        value.extend_from_slice(property.as_bytes());
        value.push(KEY_SEPARATOR);
        value.extend_from_slice(child.as_bytes());
        value
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

    /// Reads a stored value of the form `{property}\x1F{child}` back into the relationship key and
    /// the child URN. Neither an NGSI-LD relationship key nor a URN holds the separator, so the first
    /// one splits them.
    fn parse_value(value: &[u8]) -> Result<(&str, Urn)> {
        let invalid = || RelationshipStoreError::InvalidKey {
            key: String::from_utf8_lossy(value).to_string(),
        };
        let mut parts = value.splitn(2, |&byte| byte == KEY_SEPARATOR);
        let (Some(property), Some(child)) = (parts.next(), parts.next()) else {
            return Err(invalid());
        };
        let property = from_utf8(property).map_err(|_| invalid())?;
        let child = from_utf8(child).map_err(|_| invalid())?.parse::<Urn>().map_err(|_| invalid())?;
        Ok((property, child))
    }

    /// Writes the edges of every fragment in `batch` into `table`.
    fn insert_edges(&self, table: &mut Table<'_, &'static [u8], &'static [u8]>, batch: &[FragmentEdges<'_>]) -> Result<()> {
        for fragment in batch {
            for (ordinal, edge) in (0_u32..).zip(fragment.edges) {
                let key = Self::make_key(&edge.parent, fragment.position, ordinal);
                let value = Self::make_value(&edge.property, &edge.child);
                table
                    .insert(key.as_slice(), value.as_slice())
                    .map_err(|e| self.redb_err(e.into(), StoreAction::Insert))?;
            }
        }
        Ok(())
    }

    fn redb_err(&self, source: redb::Error, action: StoreAction) -> RelationshipStoreError {
        RelationshipStoreError::RedbOperation {
            source: Box::new(source),
            path: self.db_path.clone(),
            action,
        }
    }
}

impl RelationshipStore for RedbRelationshipStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::TransactionalBatch
    }

    fn add_edges(&self, edges: FragmentEdges<'_>) -> Result<()> {
        self.add_edge_batch(&[edges])
    }

    fn add_edge_batch(&self, batch: &[FragmentEdges<'_>]) -> Result<()> {
        if batch.iter().all(|fragment| fragment.edges.is_empty()) {
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
            self.insert_edges(&mut table, batch)?;
        }
        write_txn.commit().map_err(|e| self.redb_err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    fn assemble_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        // Read-only scan: redb read transactions are MVCC and lock-free, so
        // `rayon::par_iter` callers can assemble entities concurrently instead of
        // serializing on the single writer.
        let prefix = Self::make_parent_prefix(parent_urn);
        let end = Self::prefix_end(&prefix);

        let read_txn = self.db.begin_read().map_err(|e| self.redb_err(e.into(), StoreAction::BeginTransaction))?;
        let table = read_txn
            .open_table(RELATIONSHIPS_TABLE)
            .map_err(|e| self.redb_err(e.into(), StoreAction::OpenTable))?;

        let range = table
            .range(prefix.as_slice()..end.as_slice())
            .map_err(|e| self.redb_err(e.into(), StoreAction::Iterate))?;
        let mut result = StoredRelationships::default();
        for entry in range {
            let (_, value) = entry.map_err(|e| self.redb_err(e.into(), StoreAction::Iterate))?;
            let (property, child_urn) = Self::parse_value(value.value())?;
            join(&mut result, property, child_urn);
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
        error::RelationshipStoreError,
        redb_store::RedbRelationshipStore,
        relationship_edge::FragmentEdges,
        store::{
            RelationshipStore,
            tests::{assert_honours_the_contract, edges, in_order, record},
        },
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    #[test]
    fn honours_the_cross_backend_contract() {
        assert_honours_the_contract(|| Box::new(RedbRelationshipStore::new().unwrap()));
    }

    #[test]
    fn a_key_holds_the_parent_then_the_position_then_the_place_among_the_fragment_s_targets() {
        let parent: Urn = "urn:ngsi-ld:Stop:S1".parse().unwrap();
        let position = SourcePosition::first_of(2).advanced(258).nth_fragment(3);

        let key = RedbRelationshipStore::make_key(&parent, position, 5);

        let mut expected = b"urn:ngsi-ld:Stop:S1\x1F".to_vec();
        expected.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0, 3]);
        expected.extend_from_slice(&[0, 0, 0, 5]);
        assert_eq!(key, expected);
    }

    #[test]
    fn the_keys_of_one_parent_sort_bytewise_in_the_order_of_the_source() {
        let parent: Urn = "urn:ngsi-ld:Stop:S1".parse().unwrap();
        let keys = [
            RedbRelationshipStore::make_key(&parent, record(0), 0),
            RedbRelationshipStore::make_key(&parent, record(0), 1),
            RedbRelationshipStore::make_key(&parent, record(0), 256),
            RedbRelationshipStore::make_key(&parent, record(0).nth_fragment(1), 0),
            RedbRelationshipStore::make_key(&parent, record(1), 0),
            RedbRelationshipStore::make_key(&parent, record(256), 0),
            RedbRelationshipStore::make_key(&parent, SourcePosition::first_of(1), 0),
        ];
        assert!(keys.is_sorted());
    }

    #[test]
    fn a_value_reads_back_into_its_key_and_its_child() {
        let child: Urn = "urn:ngsi-ld:Airport:A1".parse().unwrap();

        let value = RedbRelationshipStore::make_value("servesAirports#1", &child);

        assert_eq!(RedbRelationshipStore::parse_value(&value).unwrap(), ("servesAirports#1", child));
    }

    #[test]
    fn a_value_without_a_separator_or_a_urn_is_an_invalid_key() {
        for value in [
            b"servedBy".as_slice(),
            b"servedBy\x1Fnot a urn".as_slice(),
            b"\xFF\x1Furn:ngsi-ld:Line:U1".as_slice(),
        ] {
            assert!(matches!(
                RedbRelationshipStore::parse_value(value),
                Err(RelationshipStoreError::InvalidKey { .. })
            ));
        }
    }

    #[test]
    fn assembling_a_parent_leaves_its_relationships_stored() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let written = edges(parent.as_str(), "hasPart", &["urn:ngsi-ld:Child:A"]);

        store
            .add_edges(FragmentEdges {
                position: record(0),
                edges: &written,
            })
            .unwrap();

        for _ in 0..2 {
            assert_eq!(in_order(&store.assemble_relationships(&parent).unwrap(), "hasPart"), ["urn:ngsi-ld:Child:A"]);
        }
    }

    #[test]
    fn prefix_scan_does_not_pick_up_other_parents() {
        let store = RedbRelationshipStore::new().unwrap();
        let parent_a: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let parent_b: Urn = "urn:ngsi-ld:Parent:12".parse().unwrap();
        let to_a = edges(parent_a.as_str(), "has", &["urn:ngsi-ld:Child:X"]);
        let to_b = edges(parent_b.as_str(), "has", &["urn:ngsi-ld:Child:Y"]);

        store
            .add_edge_batch(&[
                FragmentEdges {
                    position: record(0),
                    edges: &to_a,
                },
                FragmentEdges {
                    position: record(1),
                    edges: &to_b,
                },
            ])
            .unwrap();

        assert_eq!(in_order(&store.assemble_relationships(&parent_a).unwrap(), "has"), ["urn:ngsi-ld:Child:X"]);
        assert_eq!(in_order(&store.assemble_relationships(&parent_b).unwrap(), "has"), ["urn:ngsi-ld:Child:Y"]);
    }
}

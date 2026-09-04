use crate::{
    entity_store::error::{EntityStoreError, Result},
    store_action::StoreAction,
};
use redb::{Database, TableDefinition};
use std::{fs, path::PathBuf, sync::Arc};

/// Target cache size per database: 256 MiB, chosen to bound the disk-store working set.
const CACHE_BYTES: usize = 256 * 1024 * 1024;

/// A shared handle to a temporary redb database backing a disk entity store.
///
/// Both the current-state and series redb stores are one physical database in a
/// [`tempfile::tempdir`](tempfile::tempdir); this owns the open, the durability-free error wrapping,
/// and the temp-directory cleanup so neither store re-implements them. Cloning `Arc`-shares the one
/// database, matching how a store clone is another handle to the same backing file.
#[derive(Debug)]
pub(crate) struct RedbDatabase {
    db: Arc<Database>,
    temp_dir_path: PathBuf,
    db_path: PathBuf,
}

impl RedbDatabase {
    /// Opens a new redb database in a fresh temporary directory.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the temporary
    /// directory or the database file cannot be created.
    pub(crate) fn open(file_name: &str) -> Result<RedbDatabase> {
        let temp_dir = tempfile::tempdir().map_err(|source| EntityStoreError::CreateTempDirectory { source })?;
        let temp_dir_path = temp_dir.keep();
        let db_path = temp_dir_path.join(file_name);

        let db = Database::builder()
            .set_cache_size(CACHE_BYTES)
            .create(&db_path)
            .map_err(|e| EntityStoreError::RedbOperation {
                source: Box::new(e.into()),
                path: db_path.clone(),
                action: StoreAction::OpenDatabase,
            })?;

        Ok(RedbDatabase {
            db: Arc::new(db),
            temp_dir_path,
            db_path,
        })
    }

    /// Materializes each table so later read-only paths can assume it exists.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the tables
    /// cannot be created.
    pub(crate) fn init_tables(&self, tables: &[TableDefinition<'static, &'static [u8], &'static [u8]>]) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(|e| self.err(e.into(), StoreAction::BeginTransaction))?;
        {
            for table in tables {
                write_txn.open_table(*table).map_err(|e| self.err(e.into(), StoreAction::CreateTable))?;
            }
        }
        write_txn.commit().map_err(|e| self.err(e.into(), StoreAction::CommitTransaction))?;
        Ok(())
    }

    /// The underlying redb database.
    pub(crate) fn database(&self) -> &Database {
        &self.db
    }

    /// Wraps a redb error with the database path and the operation being attempted.
    pub(crate) fn err(&self, source: redb::Error, action: StoreAction) -> EntityStoreError {
        EntityStoreError::RedbOperation {
            source: Box::new(source),
            path: self.db_path.clone(),
            action,
        }
    }

    /// Removes the temporary directory and its database file.
    pub(crate) fn remove_files(&self) {
        let _ = fs::remove_dir_all(&self.temp_dir_path);
    }
}

impl Clone for RedbDatabase {
    fn clone(&self) -> RedbDatabase {
        RedbDatabase {
            db: Arc::clone(&self.db),
            temp_dir_path: self.temp_dir_path.clone(),
            db_path: self.db_path.clone(),
        }
    }
}

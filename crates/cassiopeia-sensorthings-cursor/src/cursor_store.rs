use crate::{
    cursor_store_error::CursorStoreError,
    cursor_update::CursorUpdate,
    positions::Positions,
    positions_table::{POSITIONS, read_positions, write_positions},
    reset::{ResetScope, ResetTarget, reset},
    store_metadata::{record, stored},
};
use cassiopeia_sensorthings::{cursor_name::CursorName, service_root::ServiceRoot};
use redb::{Database, DatabaseError, ReadableDatabase, StorageError};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

/// The extension of a store's file.
pub(crate) const STORE_EXTENSION: &str = "redb";

/// The cursor store of one SensorThings input: the read positions of its data streams, in one redb
/// database, `<folder>/<cursor>.redb`.
///
/// An open store holds its file exclusively, against every other open in this process and in any
/// other, until it is dropped, so two runs never advance one cursor. A commit is durable once it
/// returns, which redb's default durability gives.
#[derive(Debug)]
pub struct CursorStore {
    /// The database.
    database: Database,
    /// The cursor the store keeps.
    cursor: CursorName,
    /// The service root the positions were read from.
    service_root: ServiceRoot,
    /// The store's file.
    path: PathBuf,
}

impl CursorStore {
    /// Opens the cursor `cursor` in `folder` for the input that reads `root`, creating the folder
    /// and the store when there is none.
    ///
    /// # Errors
    ///
    /// Returns [`CursorStoreError::InUse`] while another handle holds the store,
    /// [`CursorStoreError::OtherKind`], [`CursorStoreError::UnsupportedFormat`] or
    /// [`CursorStoreError::OtherServiceRoot`] for a store of another kind of input, another format
    /// or another server, [`CursorStoreError::NotACursorStore`] for a file that is no cursor store,
    /// [`CursorStoreError::CreateFolder`] when the folder cannot be created, and
    /// [`CursorStoreError::Storage`] when the database fails.
    pub fn open(folder: &Path, cursor: CursorName, root: ServiceRoot) -> Result<CursorStore, CursorStoreError> {
        fs::create_dir_all(folder).map_err(|source| CursorStoreError::CreateFolder {
            folder: folder.to_owned(),
            source,
        })?;
        let path = store_path(folder, &cursor);
        let database = Database::create(&path).map_err(|error| refusal(error, &cursor))?;
        match recorded_root(&database, &cursor, &path)? {
            Some(stored) if stored != root => {
                return Err(CursorStoreError::OtherServiceRoot {
                    cursor,
                    stored: Box::new(stored),
                    requested: Box::new(root),
                });
            }
            Some(_) => {}
            None => initialize(&database, &root, &cursor)?,
        }
        Ok(CursorStore {
            database,
            cursor,
            service_root: root,
            path,
        })
    }

    /// Opens the cursor `cursor` in `folder`, which must already exist, whatever service root it
    /// records, as an inspection does.
    ///
    /// # Errors
    ///
    /// Returns [`CursorStoreError::NotFound`] when `folder` holds no store for `cursor`, and the
    /// errors of [`CursorStore::open`] but [`CursorStoreError::OtherServiceRoot`] and
    /// [`CursorStoreError::CreateFolder`].
    pub fn open_existing(folder: &Path, cursor: CursorName) -> Result<CursorStore, CursorStoreError> {
        let path = store_path(folder, &cursor);
        let database = match Database::open(&path) {
            Ok(database) => database,
            Err(DatabaseError::Storage(StorageError::Io(error))) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CursorStoreError::NotFound {
                    cursor,
                    folder: folder.to_owned(),
                });
            }
            Err(other) => return Err(refusal(other, &cursor)),
        };
        let Some(service_root) = recorded_root(&database, &cursor, &path)? else {
            return Err(CursorStoreError::NotACursorStore { cursor, path });
        };
        Ok(CursorStore {
            database,
            cursor,
            service_root,
            path,
        })
    }

    /// The cursor the store keeps.
    #[must_use]
    pub const fn cursor(&self) -> &CursorName {
        &self.cursor
    }

    /// The service root the store's positions were read from.
    #[must_use]
    pub const fn service_root(&self) -> &ServiceRoot {
        &self.service_root
    }

    /// The store's file, `<folder>/<cursor>.redb`.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The positions the store holds.
    ///
    /// # Errors
    ///
    /// Returns [`CursorStoreError::UnreadablePosition`] when a position cannot be decoded, and
    /// [`CursorStoreError::Storage`] when the database fails.
    pub fn positions(&self) -> Result<Positions, CursorStoreError> {
        let transaction = self.database.begin_read().map_err(CursorStoreError::storage(&self.cursor))?;
        let table = transaction.open_table(POSITIONS).map_err(CursorStoreError::storage(&self.cursor))?;
        read_positions(&table, &self.cursor)
    }

    /// Moves the positions the store holds past `update`, in one write transaction.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`CursorStore::positions`], and [`CursorStoreError::UnwritablePosition`]
    /// when a position cannot be encoded. The store is then left as it was.
    pub fn commit(&self, update: CursorUpdate) -> Result<(), CursorStoreError> {
        self.rewrite(|positions| Ok(update.apply(positions)))
    }

    /// Resets the positions within `scope` to `target`, in one write transaction.
    ///
    /// # Errors
    ///
    /// Returns [`CursorStoreError::UnknownDatastream`] when `scope` names a data stream the store
    /// holds no position for, and the errors of [`CursorStore::commit`]. The store is then left as it
    /// was.
    pub fn reset(&self, scope: &ResetScope, target: ResetTarget) -> Result<(), CursorStoreError> {
        self.rewrite(|positions| reset(positions, scope, target, &self.cursor))
    }

    /// Replaces the positions the store holds with what `change` makes of them, in one write
    /// transaction, which is dropped, and so changes nothing, when `change` fails.
    fn rewrite(&self, change: impl FnOnce(Positions) -> Result<Positions, CursorStoreError>) -> Result<(), CursorStoreError> {
        let transaction = self.database.begin_write().map_err(CursorStoreError::storage(&self.cursor))?;
        {
            let mut table = transaction.open_table(POSITIONS).map_err(CursorStoreError::storage(&self.cursor))?;
            let changed = change(read_positions(&table, &self.cursor)?)?;
            write_positions(&mut table, changed, &self.cursor)?;
        }
        transaction.commit().map_err(CursorStoreError::storage(&self.cursor))
    }
}

/// The file of the cursor `cursor` in `folder`.
fn store_path(folder: &Path, cursor: &CursorName) -> PathBuf {
    folder.join(cursor.as_str()).with_extension(STORE_EXTENSION)
}

/// What an open of the store of `cursor` that `error` stopped is refused as: in use while another
/// handle holds the file.
fn refusal(error: DatabaseError, cursor: &CursorName) -> CursorStoreError {
    // A handle of another redb version holds the file under a locking protocol of its own. The
    // error enum is non-exhaustive, so every other failure, a later one included, is a database
    // failure.
    if matches!(error, DatabaseError::DatabaseAlreadyOpen | DatabaseError::LockProtocolMismatch) {
        return CursorStoreError::InUse {
            // Each error owns the name of the cursor it names, and outlives the store.
            cursor: cursor.clone(),
        };
    }
    CursorStoreError::storage(cursor)(error)
}

/// The service root `database`, the store of `cursor` at `path`, records, or `None` for a store that
/// holds no table at all, as one created a moment ago.
///
/// # Errors
///
/// Returns the refusals a store's recorded kind, format and service root are checked with,
/// [`CursorStoreError::NotACursorStore`] for a database that holds tables but no metadata, and
/// [`CursorStoreError::Storage`] when the database fails.
fn recorded_root(database: &Database, cursor: &CursorName, path: &Path) -> Result<Option<ServiceRoot>, CursorStoreError> {
    let transaction = database.begin_read().map_err(CursorStoreError::storage(cursor))?;
    if let Some(metadata) = stored(&transaction).map_err(CursorStoreError::storage(cursor))? {
        return metadata.service_root(cursor, path).map(Some);
    }
    let holds_tables = transaction.list_tables().map_err(CursorStoreError::storage(cursor))?.next().is_some()
        || transaction.list_multimap_tables().map_err(CursorStoreError::storage(cursor))?.next().is_some();
    if holds_tables {
        return Err(CursorStoreError::NotACursorStore {
            // Each error owns the name of the cursor it names, and outlives the store.
            cursor: cursor.clone(),
            path: path.to_owned(),
        });
    }
    Ok(None)
}

/// Records in `database`, the new store of `cursor`, its kind, its format and `root`, and creates
/// its positions table, in one write transaction.
fn initialize(database: &Database, root: &ServiceRoot, cursor: &CursorName) -> Result<(), CursorStoreError> {
    let transaction = database.begin_write().map_err(CursorStoreError::storage(cursor))?;
    record(&transaction, root).map_err(CursorStoreError::storage(cursor))?;
    transaction.open_table(POSITIONS).map_err(CursorStoreError::storage(cursor))?;
    transaction.commit().map_err(CursorStoreError::storage(cursor))
}

#[cfg(test)]
mod tests {
    use crate::{
        cursor_store::CursorStore,
        cursor_store_error::CursorStoreError,
        cursor_update::CursorUpdate,
        positions::Positions,
        reset::{IdText, ResetScope, ResetTarget},
        seen_observations::SeenObservations,
        store_metadata::{METADATA, MetadataKey},
        test_metadata::rewrite_metadata,
    };
    use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, service_root::ServiceRoot};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use redb::{Database, ReadableDatabase, TableDefinition};
    use std::{collections::BTreeMap, time::Duration};
    use tempfile::TempDir;

    const ROOT_A: &str = "https://api-samenmeten.rivm.nl/v1.0";
    const ROOT_B: &str = "https://iot.example.org/v1.1";
    const LOOKBACK: Duration = Duration::from_secs(3600);

    fn cursor(name: &str) -> CursorName {
        CursorName::try_from(name.to_owned()).unwrap()
    }

    fn root(text: &str) -> ServiceRoot {
        text.parse().unwrap()
    }

    /// The instant `time` on 2026-11-12, in UTC.
    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
    }

    fn seen(observations: &[(i64, &str)]) -> SeenObservations {
        SeenObservations::from(
            observations
                .iter()
                .map(|(id, time)| (EntityId::Integer(*id), at(time)))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn open(folder: &TempDir) -> Result<CursorStore, CursorStoreError> {
        CursorStore::open(folder.path(), cursor("rivm-air"), root(ROOT_A))
    }

    /// The message `outcome` failed with.
    fn refusal<T>(outcome: Result<T, CursorStoreError>) -> String {
        outcome.err().map(|error| error.to_string()).unwrap_or_default()
    }

    /// A store `rivm-air` for root A holding positions for the data streams 5 and 9.
    fn store_with_streams_5_and_9(folder: &TempDir) -> CursorStore {
        let store = open(folder).unwrap();
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(EntityId::Integer(5), seen(&[(50, "09:30")]));
        update.record(EntityId::Integer(9), seen(&[(90, "09:40")]));
        store.commit(update).unwrap();
        store
    }

    #[test]
    fn a_new_store_records_its_kind_format_and_service_root() {
        let folder = tempfile::tempdir().unwrap();
        let nested = folder.path().join("cursors");
        drop(CursorStore::open(&nested, cursor("rivm-air"), root(ROOT_A)).unwrap());

        let database = Database::open(nested.join("rivm-air.redb")).unwrap();
        let transaction = database.begin_read().unwrap();
        let metadata = transaction.open_table(METADATA).unwrap();
        let recorded = |key: MetadataKey| metadata.get(<&str>::from(key)).unwrap().unwrap().value().to_owned();

        assert_eq!(
            [recorded(MetadataKey::Kind), recorded(MetadataKey::Format), recorded(MetadataKey::ServiceRoot)],
            ["sensorthings", "1", ROOT_A]
        );
    }

    #[test]
    fn reopening_for_another_server_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        drop(open(&folder).unwrap());

        assert_eq!(
            refusal(CursorStore::open(folder.path(), cursor("rivm-air"), root(ROOT_B))),
            format!("cursor 'rivm-air' keeps the read positions of '{ROOT_A}', not of '{ROOT_B}': name another cursor for '{ROOT_B}'")
        );
    }

    #[test]
    fn a_second_handle_is_refused_while_one_is_open() {
        let folder = tempfile::tempdir().unwrap();
        let held = open(&folder).unwrap();

        assert_eq!(refusal(open(&folder)), "cursor 'rivm-air' is in use by a running job");
        assert_eq!(
            refusal(CursorStore::open_existing(folder.path(), cursor("rivm-air"))),
            "cursor 'rivm-air' is in use by a running job"
        );
        drop(held);
        assert!(open(&folder).is_ok());
    }

    #[test]
    fn committed_positions_survive_reopening() {
        let folder = tempfile::tempdir().unwrap();
        let store = open(&folder).unwrap();
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(EntityId::Integer(5), seen(&[(50, "09:30")]));
        update.record(
            EntityId::Text("t-5".into()),
            SeenObservations::from(BTreeMap::from([(EntityId::Text("o-1".into()), at("09:45"))])),
        );
        store.commit(update).unwrap();
        let committed = store.positions().unwrap();
        drop(store);

        assert_eq!(committed.len(), 2);
        assert_eq!(open(&folder).unwrap().positions().unwrap(), committed);
    }

    #[test]
    fn an_update_never_committed_changes_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_with_streams_5_and_9(&folder);
        let committed = store.positions().unwrap();
        let mut update = CursorUpdate::new(at("11:00"), LOOKBACK);
        update.record(EntityId::Integer(5), seen(&[(51, "10:30")]));
        drop(update);
        drop(store);

        assert_eq!(open(&folder).unwrap().positions().unwrap(), committed);
    }

    #[test]
    fn commit_prunes_seen_observations_behind_the_lookback() {
        let folder = tempfile::tempdir().unwrap();
        let store = open(&folder).unwrap();
        let mut update = CursorUpdate::new(at("10:00"), LOOKBACK);
        update.record(EntityId::Integer(5), seen(&[(1, "08:30"), (2, "09:10"), (3, "10:00")]));
        store.commit(update).unwrap();
        let positions = store.positions().unwrap();
        let position = positions.get(&EntityId::Integer(5)).unwrap();

        assert_eq!(position.high_water(), at("10:00"));
        assert_eq!(position.seen(), &seen(&[(2, "09:10"), (3, "10:00")]));
    }

    #[test]
    fn reset_removes_every_position() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_with_streams_5_and_9(&folder);
        store.reset(&ResetScope::Store, ResetTarget::InitialPoint).unwrap();

        assert_eq!(store.positions().unwrap(), Positions::default());
    }

    #[test]
    fn reset_of_one_stream_keeps_the_others() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_with_streams_5_and_9(&folder);
        let before = store.positions().unwrap();
        store
            .reset(&ResetScope::Datastream(IdText::from(Box::<str>::from("5"))), ResetTarget::InitialPoint)
            .unwrap();
        let after = store.positions().unwrap();

        assert_eq!(after.len(), 1);
        assert_eq!(after.get(&EntityId::Integer(9)), before.get(&EntityId::Integer(9)));
    }

    #[test]
    fn reset_to_an_instant_sets_high_water_and_clears_seen() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_with_streams_5_and_9(&folder);
        let before = store.positions().unwrap();
        store
            .reset(
                &ResetScope::Datastream(IdText::from(Box::<str>::from("5"))),
                ResetTarget::HighWater(at("08:00")),
            )
            .unwrap();
        let after = store.positions().unwrap();
        let rewound = after.get(&EntityId::Integer(5)).unwrap();

        assert_eq!(rewound.high_water(), at("08:00"));
        assert!(rewound.seen().is_empty());
        assert_eq!(after.get(&EntityId::Integer(9)), before.get(&EntityId::Integer(9)));
    }

    #[test]
    fn reset_to_an_instant_applies_to_every_stream() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_with_streams_5_and_9(&folder);
        store.reset(&ResetScope::Store, ResetTarget::HighWater(at("08:00"))).unwrap();
        let after = store.positions().unwrap();

        for datastream in [EntityId::Integer(5), EntityId::Integer(9)] {
            let rewound = after.get(&datastream).unwrap();

            assert_eq!((rewound.high_water(), rewound.seen().is_empty()), (at("08:00"), true));
        }
    }

    #[test]
    fn resetting_an_unknown_stream_names_it() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_with_streams_5_and_9(&folder);
        let before = store.positions().unwrap();

        assert_eq!(
            refusal(store.reset(&ResetScope::Datastream(IdText::from(Box::<str>::from("42"))), ResetTarget::InitialPoint)),
            "cursor 'rivm-air' holds no position for the data stream '42'"
        );
        assert_eq!(store.positions().unwrap(), before);
    }

    #[test]
    fn a_store_of_another_kind_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        drop(open(&folder).unwrap());
        rewrite_metadata(&folder.path().join("rivm-air.redb"), MetadataKey::Kind, "other");

        assert_eq!(
            refusal(open(&folder)),
            "cursor 'rivm-air' belongs to an input of kind 'other', not to a SensorThings input"
        );
    }

    #[test]
    fn an_unknown_store_format_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        drop(open(&folder).unwrap());
        rewrite_metadata(&folder.path().join("rivm-air.redb"), MetadataKey::Format, "2");

        assert_eq!(
            refusal(open(&folder)),
            "cursor 'rivm-air' is stored in format 2, and this version of Cassiopeia reads format 1"
        );
    }

    #[test]
    fn a_file_that_is_no_cursor_store_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("rivm-air.redb");
        let database = Database::create(&path).unwrap();
        let transaction = database.begin_write().unwrap();
        transaction
            .open_table(TableDefinition::<&str, u64>::new("other"))
            .unwrap()
            .insert("x", 1)
            .unwrap();
        transaction.commit().unwrap();
        drop(database);

        let expected = format!("cursor 'rivm-air' cannot be opened: '{}' is no cursor store", path.display());
        assert_eq!(refusal(open(&folder)), expected);
        assert_eq!(refusal(CursorStore::open_existing(folder.path(), cursor("rivm-air"))), expected);
    }

    #[test]
    fn an_absent_cursor_is_not_found() {
        let folder = tempfile::tempdir().unwrap();

        assert_eq!(
            refusal(CursorStore::open_existing(folder.path(), cursor("nope"))),
            format!("cursor 'nope' does not exist in '{}'", folder.path().display())
        );
        assert!(!folder.path().join("nope.redb").exists());
    }

    #[test]
    fn an_existing_cursor_reads_its_service_root() {
        let folder = tempfile::tempdir().unwrap();
        drop(open(&folder).unwrap());
        let store = CursorStore::open_existing(folder.path(), cursor("rivm-air")).unwrap();

        assert_eq!(store.service_root(), &root(ROOT_A));
        assert_eq!(store.cursor(), &cursor("rivm-air"));
        assert_eq!(store.path(), folder.path().join("rivm-air.redb"));
    }
}

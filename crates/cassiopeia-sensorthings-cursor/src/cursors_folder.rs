use crate::{
    cursor_store::{CursorStore, STORE_EXTENSION},
    cursor_store_error::CursorStoreError,
};
use cassiopeia_sensorthings::cursor_name::CursorName;
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    io,
    path::{Path, PathBuf},
};

/// The SensorThings cursor stores in `folder`, by ascending cursor name, each opened as
/// [`CursorStore::open_existing`] opens it when the iterator reaches it, so a caller that drops each
/// store before the next holds one at a time.
///
/// Every `<cursor>.redb` file in `folder` is a store. A store of another kind of input is left out,
/// and so is a file whose name names no cursor, since no cursor store has such a name. A folder that
/// does not exist holds no store, as before the first incremental run.
///
/// # Errors
///
/// Returns [`CursorStoreError::ReadFolder`] when the folder cannot be read. Each store is then
/// opened with the errors of [`CursorStore::open_existing`], but [`CursorStoreError::OtherKind`].
pub fn sensorthings_stores(folder: &Path) -> Result<impl Iterator<Item = Result<CursorStore, CursorStoreError>>, CursorStoreError> {
    Ok(stored_cursors(folder)?
        .into_iter()
        .filter_map(|cursor| match CursorStore::open_existing(folder, cursor) {
            Ok(store) => Some(Ok(store)),
            // Another kind of input keeps positions a SensorThings input does not read.
            Err(CursorStoreError::OtherKind { .. }) => None,
            Err(refusal) => Some(Err(refusal)),
        }))
}

/// The cursors whose stores `folder` holds, of every kind of input.
fn stored_cursors(folder: &Path) -> Result<BTreeSet<CursorName>, CursorStoreError> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => return Err(unreadable(folder, source)),
    };
    entries
        .map(|entry| entry.map(|entry| cursor_of(&entry.path())).map_err(|source| unreadable(folder, source)))
        .filter_map(Result::transpose)
        .collect()
}

/// The cursor whose store the file at `path` is, or `None` when it is none.
fn cursor_of(path: &Path) -> Option<CursorName> {
    if path.extension() != Some(OsStr::new(STORE_EXTENSION)) || !path.is_file() {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

/// The refusal of `folder`, which the operating system failed to read with `source`.
fn unreadable(folder: &Path, source: io::Error) -> CursorStoreError {
    CursorStoreError::ReadFolder {
        folder: PathBuf::from(folder),
        source,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cursor_store::CursorStore,
        cursor_store_error::CursorStoreError,
        cursors_folder::sensorthings_stores,
        store_metadata::MetadataKey,
        test_metadata::rewrite_metadata,
    };
    use cassiopeia_sensorthings::{cursor_name::CursorName, service_root::ServiceRoot};
    use pretty_assertions::assert_eq;
    use std::{fs, path::Path};
    use tempfile::TempDir;

    const ROOT: &str = "https://api-samenmeten.rivm.nl/v1.0";

    /// Creates the SensorThings store `name` in `folder`, holding no position.
    fn create(folder: &Path, name: &str) {
        let root: ServiceRoot = ROOT.parse().unwrap();
        drop(CursorStore::open(folder, name.parse().unwrap(), root).unwrap());
    }

    /// The names of the stores `folder` lists, or the message the first refused store failed with.
    fn listed(folder: &Path) -> Result<Vec<String>, String> {
        sensorthings_stores(folder)
            .map_err(|refusal| refusal.to_string())?
            .map(|store| store.map(|store| store.cursor().to_string()).map_err(|refusal| refusal.to_string()))
            .collect()
    }

    #[test]
    fn stores_are_listed_by_name_and_one_of_another_kind_is_left_out() {
        let folder = TempDir::new().unwrap();
        for name in ["b", "a", "other-kind"] {
            create(folder.path(), name);
        }
        rewrite_metadata(&folder.path().join("other-kind.redb"), MetadataKey::Kind, "other");

        assert_eq!(listed(folder.path()), Ok(vec!["a".to_owned(), "b".to_owned()]));
    }

    #[test]
    fn a_file_that_names_no_cursor_is_left_out() {
        let folder = TempDir::new().unwrap();
        create(folder.path(), "a");
        fs::write(folder.path().join("Notes.redb"), "").unwrap();
        fs::write(folder.path().join("b.txt"), "").unwrap();
        fs::create_dir(folder.path().join("c.redb")).unwrap();

        assert_eq!(listed(folder.path()), Ok(vec!["a".to_owned()]));
    }

    #[test]
    fn a_folder_that_does_not_exist_holds_no_store() {
        let folder = TempDir::new().unwrap();

        assert_eq!(listed(&folder.path().join("cursors")), Ok(Vec::new()));
    }

    #[test]
    fn a_store_in_use_is_refused_as_opening_it_is() {
        let folder = TempDir::new().unwrap();
        let root: ServiceRoot = ROOT.parse().unwrap();
        let held = CursorStore::open(folder.path(), CursorName::try_from("a".to_owned()).unwrap(), root).unwrap();

        assert_eq!(listed(folder.path()), Err("cursor 'a' is in use by a running job".to_owned()));
        drop(held);
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_named() {
        let folder = TempDir::new().unwrap();
        let file = folder.path().join("cursors");
        fs::write(&file, "").unwrap();

        let refusal = sensorthings_stores(&file).err().unwrap();

        assert!(matches!(refusal, CursorStoreError::ReadFolder { .. }), "{refusal:?}");
        assert_eq!(refusal.to_string(), format!("the cursors folder '{}' cannot be read", file.display()));
    }
}

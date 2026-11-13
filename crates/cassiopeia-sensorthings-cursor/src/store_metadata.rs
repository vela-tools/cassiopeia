use crate::cursor_store_error::CursorStoreError;
use cassiopeia_sensorthings::{cursor_name::CursorName, service_root::ServiceRoot};
use redb::{ReadTransaction, TableDefinition, TableError, WriteTransaction};
use std::path::Path;
use strum::IntoStaticStr;

/// The table of what a store records about itself: the kind of input it belongs to, its format and
/// the service root its positions were read from, each as text, so any kind of input reads a store's
/// kind without knowing its format.
pub(crate) const METADATA: TableDefinition<&str, &str> = TableDefinition::new("metadata");

/// The kind of input a SensorThings store records.
const SENSORTHINGS_KIND: &str = "sensorthings";

/// The store format this version of Cassiopeia reads and writes.
const STORE_FORMAT: &str = "1";

/// What a store records about itself, each under its own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum MetadataKey {
    /// The kind of input the store belongs to.
    Kind,
    /// The store format.
    Format,
    /// The service root the positions were read from.
    ServiceRoot,
}

/// What a store records about itself, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredMetadata {
    /// The kind of input, or `None` when the store records none.
    kind: Option<Box<str>>,
    /// The store format, or `None` when the store records none.
    format: Option<Box<str>>,
    /// The service root, or `None` when the store records none.
    service_root: Option<Box<str>>,
}

/// Records, in a new store, that it keeps the positions of a SensorThings input read from `root`, in
/// format 1.
pub(crate) fn record(transaction: &WriteTransaction, root: &ServiceRoot) -> Result<(), redb::Error> {
    let mut table = transaction.open_table(METADATA)?;
    table.insert(<&str>::from(MetadataKey::Kind), SENSORTHINGS_KIND)?;
    table.insert(<&str>::from(MetadataKey::Format), STORE_FORMAT)?;
    table.insert(<&str>::from(MetadataKey::ServiceRoot), root.url().as_str())?;
    Ok(())
}

/// What the store `transaction` reads records about itself, or `None` when it has no metadata table,
/// as a store created a moment ago.
pub(crate) fn stored(transaction: &ReadTransaction) -> Result<Option<StoredMetadata>, redb::Error> {
    let table = match transaction.open_table(METADATA) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(other) => return Err(other.into()),
    };
    let text = |key: MetadataKey| -> Result<Option<Box<str>>, redb::Error> { Ok(table.get(<&str>::from(key))?.map(|value| Box::from(value.value()))) };
    Ok(Some(StoredMetadata {
        kind: text(MetadataKey::Kind)?,
        format: text(MetadataKey::Format)?,
        service_root: text(MetadataKey::ServiceRoot)?,
    }))
}

impl StoredMetadata {
    /// The service root a SensorThings store of format 1 records, for the cursor `cursor` whose file
    /// is `path`.
    ///
    /// The kind is checked before the format, because another kind of input numbers its formats its
    /// own way.
    ///
    /// # Errors
    ///
    /// Returns [`CursorStoreError::OtherKind`] for a store of another kind of input,
    /// [`CursorStoreError::UnsupportedFormat`] for a store of another format,
    /// [`CursorStoreError::UnreadableServiceRoot`] for a recorded root that is none, and
    /// [`CursorStoreError::NotACursorStore`] for a store that records none of these.
    pub(crate) fn service_root(self, cursor: &CursorName, path: &Path) -> Result<ServiceRoot, CursorStoreError> {
        // Each error owns the name of the cursor it names, and outlives the store.
        let not_a_store = || CursorStoreError::NotACursorStore {
            cursor: cursor.clone(),
            path: path.to_owned(),
        };
        match self.kind {
            None => return Err(not_a_store()),
            Some(kind) if &*kind != SENSORTHINGS_KIND => {
                return Err(CursorStoreError::OtherKind { cursor: cursor.clone(), kind });
            }
            Some(_) => {}
        }
        match self.format {
            None => return Err(not_a_store()),
            Some(format) if &*format != STORE_FORMAT => {
                return Err(CursorStoreError::UnsupportedFormat {
                    cursor: cursor.clone(),
                    format,
                });
            }
            Some(_) => {}
        }
        self.service_root
            .ok_or_else(not_a_store)?
            .parse()
            .map_err(|source| CursorStoreError::UnreadableServiceRoot {
                cursor: cursor.clone(),
                source,
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::store_metadata::{MetadataKey, StoredMetadata};
    use cassiopeia_sensorthings::cursor_name::CursorName;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    const ROOT: &str = "https://api-samenmeten.rivm.nl/v1.0";

    fn metadata(kind: Option<&str>, format: Option<&str>, service_root: Option<&str>) -> StoredMetadata {
        StoredMetadata {
            kind: kind.map(Box::from),
            format: format.map(Box::from),
            service_root: service_root.map(Box::from),
        }
    }

    fn checked(stored: StoredMetadata) -> Result<String, String> {
        let cursor = CursorName::try_from("rivm-air".to_owned()).unwrap();
        stored
            .service_root(&cursor, Path::new("/cursors/rivm-air.redb"))
            .map(|root| root.to_string())
            .map_err(|refusal| refusal.to_string())
    }

    #[test]
    fn metadata_keys_are_written_in_kebab_case() {
        assert_eq!(<&str>::from(MetadataKey::ServiceRoot), "service-root");
        assert_eq!(<&str>::from(MetadataKey::Kind), "kind");
    }

    #[test]
    fn a_sensorthings_store_of_format_one_gives_its_root() {
        assert_eq!(checked(metadata(Some("sensorthings"), Some("1"), Some(ROOT))), Ok(ROOT.to_owned()));
    }

    #[test]
    fn the_kind_is_checked_before_the_format() {
        assert_eq!(
            checked(metadata(Some("other"), Some("7"), None)),
            Err("cursor 'rivm-air' belongs to an input of kind 'other', not to a SensorThings input".to_owned())
        );
    }

    #[test]
    fn a_missing_record_is_no_cursor_store_and_a_malformed_root_is_named() {
        let not_a_store = Err("cursor 'rivm-air' cannot be opened: '/cursors/rivm-air.redb' is no cursor store".to_owned());

        assert_eq!(checked(metadata(None, Some("1"), Some(ROOT))), not_a_store);
        assert_eq!(checked(metadata(Some("sensorthings"), None, Some(ROOT))), not_a_store);
        assert_eq!(checked(metadata(Some("sensorthings"), Some("1"), None)), not_a_store);
        assert_eq!(
            checked(metadata(Some("sensorthings"), Some("1"), Some("https://h.example/api"))),
            Err("cursor 'rivm-air' records a service root that is none".to_owned())
        );
    }
}

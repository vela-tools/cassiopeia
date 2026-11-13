use crate::reset::IdText;
use cassiopeia_sensorthings::{
    cursor_name::CursorName,
    service_root::{ServiceRoot, ServiceRootError},
};
use rmp_serde::{decode, encode};
use std::{io, path::PathBuf};
use thiserror::Error;

/// Why a cursor store cannot be opened, read or written.
#[derive(Debug, Error)]
pub enum CursorStoreError {
    /// The cursors folder cannot be created.
    #[error("the cursors folder '{}' cannot be created", folder.display())]
    CreateFolder {
        /// The folder.
        folder: PathBuf,
        /// The failure the operating system reported.
        #[source]
        source: io::Error,
    },

    /// Another handle holds the store, in this process or another.
    #[error("cursor '{cursor}' is in use by a running job")]
    InUse {
        /// The cursor.
        cursor: CursorName,
    },

    /// The cursor must already exist, and the cursors folder holds no store for it.
    #[error("cursor '{cursor}' does not exist in '{}'", folder.display())]
    NotFound {
        /// The cursor.
        cursor: CursorName,
        /// The cursors folder.
        folder: PathBuf,
    },

    /// The store keeps the positions of another kind of input.
    #[error("cursor '{cursor}' belongs to an input of kind '{kind}', not to a SensorThings input")]
    OtherKind {
        /// The cursor.
        cursor: CursorName,
        /// The kind of input the store records.
        kind: Box<str>,
    },

    /// The store records a format this version of Cassiopeia does not read.
    #[error("cursor '{cursor}' is stored in format {format}, and this version of Cassiopeia reads format 1")]
    UnsupportedFormat {
        /// The cursor.
        cursor: CursorName,
        /// The format the store records.
        format: Box<str>,
    },

    /// The store keeps the positions of another server.
    #[error("cursor '{cursor}' keeps the read positions of '{stored}', not of '{requested}': name another cursor for '{requested}'")]
    OtherServiceRoot {
        /// The cursor.
        cursor: CursorName,
        /// The service root the store records. Boxed so a root does not enlarge every `Result` of
        /// the store.
        stored: Box<ServiceRoot>,
        /// The service root the store was opened for.
        requested: Box<ServiceRoot>,
    },

    /// The file at the store's path holds no cursor store.
    #[error("cursor '{cursor}' cannot be opened: '{}' is no cursor store", path.display())]
    NotACursorStore {
        /// The cursor.
        cursor: CursorName,
        /// The file.
        path: PathBuf,
    },

    /// The store records a service root that is none.
    #[error("cursor '{cursor}' records a service root that is none")]
    UnreadableServiceRoot {
        /// The cursor.
        cursor: CursorName,
        /// Why the recorded root is none.
        #[source]
        source: ServiceRootError,
    },

    /// A position the store holds cannot be decoded.
    #[error("cursor '{cursor}' holds a position that cannot be read")]
    UnreadablePosition {
        /// The cursor.
        cursor: CursorName,
        /// Why the position cannot be decoded.
        #[source]
        source: decode::Error,
    },

    /// A position cannot be encoded.
    #[error("a position of cursor '{cursor}' cannot be written")]
    UnwritablePosition {
        /// The cursor.
        cursor: CursorName,
        /// Why the position cannot be encoded.
        #[source]
        source: encode::Error,
    },

    /// A reset names a data stream the store holds no position for.
    #[error("cursor '{cursor}' holds no position for the data stream '{datastream}'")]
    UnknownDatastream {
        /// The cursor.
        cursor: CursorName,
        /// The data stream id, as the reset named it.
        datastream: IdText,
    },

    /// The database failed.
    #[error("cursor '{cursor}' cannot be read or written")]
    Storage {
        /// The cursor.
        cursor: CursorName,
        /// The database failure, boxed because a `redb::Error` would enlarge every `Result` of the
        /// store.
        #[source]
        source: Box<redb::Error>,
    },
}

impl CursorStoreError {
    /// What a database failure of the store of `cursor` is reported as.
    pub(crate) fn storage<E: Into<redb::Error>>(cursor: &CursorName) -> impl Fn(E) -> CursorStoreError + '_ {
        |error| CursorStoreError::Storage {
            // Each error owns the name of the cursor it names, and outlives the store.
            cursor: cursor.clone(),
            source: Box::new(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cursor_store_error::CursorStoreError;
    use cassiopeia_sensorthings::cursor_name::CursorName;
    use pretty_assertions::assert_eq;
    use redb::StorageError;
    use std::error::Error;

    #[test]
    fn a_database_failure_names_the_cursor_and_keeps_its_cause() {
        let cursor = CursorName::try_from("rivm-air".to_owned()).unwrap();
        let error = CursorStoreError::storage(&cursor)(StorageError::DatabaseClosed);

        assert_eq!(error.to_string(), "cursor 'rivm-air' cannot be read or written");
        assert!(error.source().is_some());
    }
}

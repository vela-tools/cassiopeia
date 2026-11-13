//! What the cursor store's tests share: rewriting what a store records about itself, as a store of
//! another kind of input or of another format records it.

use crate::store_metadata::{METADATA, MetadataKey};
use redb::Database;
use std::path::Path;

/// Writes `value` under `key` in the metadata of the store at `path`.
pub(crate) fn rewrite_metadata(path: &Path, key: MetadataKey, value: &str) {
    let database = Database::open(path).unwrap();
    let transaction = database.begin_write().unwrap();
    transaction.open_table(METADATA).unwrap().insert(<&str>::from(key), value).unwrap();
    transaction.commit().unwrap();
}

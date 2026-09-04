use crate::{
    entity_store::{error::Result, redb_database::RedbDatabase},
    store_action::StoreAction,
};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use std::mem;
use urn_rs::Urn;

/// Unit separator delimiting the base id from a key's trailing discriminant (a big-endian mapping id
/// or sequence number). A URN never contains a control byte, so the first `0x1F` unambiguously ends
/// the base id.
pub(crate) const KEY_SEPARATOR: u8 = 0x1F;

/// The prefix `{base_id}\x1F` shared by every key belonging to one base id.
pub(crate) fn make_prefix(base_id: &Urn) -> Vec<u8> {
    let base = base_id.as_str();
    let mut prefix = Vec::with_capacity(base.len() + 1);
    prefix.extend_from_slice(base.as_bytes());
    prefix.push(KEY_SEPARATOR);
    prefix
}

/// An exclusive upper bound for a prefix scan, incrementing the trailing separator. The separator is
/// `0x1F`, so the increment never carries.
pub(crate) fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut end = prefix.to_vec();
    if let Some(last) = end.last_mut() {
        *last = last.saturating_add(1);
    }
    end
}

/// The base id bytes of a composite key: everything up to the first separator.
pub(crate) fn base_id_bytes(key: &[u8]) -> &[u8] {
    match key.iter().position(|&byte| byte == KEY_SEPARATOR) {
        Some(position) => &key[..position],
        None => key,
    }
}

/// Streams the distinct base ids in `table` chunk by chunk.
///
/// Keys sort with the separator ahead of any URN character, so every key of one base id is contiguous
/// and no shorter base id's keys interleave a longer one's; tracking the previous base id across the
/// sorted scan therefore yields each id once.
pub(crate) fn for_each_id_chunk(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    chunk_size: usize,
    callback: &mut dyn FnMut(Vec<Urn>) -> Result<()>,
) -> Result<()> {
    let read_txn = db.database().begin_read().map_err(|e| db.err(e.into(), StoreAction::BeginTransaction))?;
    let table = read_txn.open_table(table).map_err(|e| db.err(e.into(), StoreAction::OpenTable))?;

    // The initial capacity is bounded so `collect_ids` can pass `usize::MAX` without a giant reserve.
    let mut chunk: Vec<Urn> = Vec::with_capacity(chunk_size.min(1024));
    let mut previous: Option<Vec<u8>> = None;
    for entry in table.iter().map_err(|e| db.err(e.into(), StoreAction::Iterate))? {
        let (key, _value) = entry.map_err(|e| db.err(e.into(), StoreAction::Iterate))?;
        let base = base_id_bytes(key.value());
        if previous.as_deref() == Some(base) {
            continue;
        }
        previous = Some(base.to_vec());
        if let Ok(urn) = String::from_utf8_lossy(base).parse::<Urn>() {
            chunk.push(urn);
            if chunk.len() >= chunk_size {
                callback(mem::take(&mut chunk))?;
            }
        }
    }
    if !chunk.is_empty() {
        callback(chunk)?;
    }
    Ok(())
}

/// Collects every distinct base id in `table`.
pub(crate) fn collect_ids(db: &RedbDatabase, table: TableDefinition<'static, &'static [u8], &'static [u8]>) -> Result<Vec<Urn>> {
    let mut ids = Vec::new();
    for_each_id_chunk(db, table, usize::MAX, &mut |chunk| {
        ids.extend(chunk);
        Ok(())
    })?;
    Ok(ids)
}

/// Counts the distinct base ids in `table`.
pub(crate) fn distinct_id_count(db: &RedbDatabase, table: TableDefinition<'static, &'static [u8], &'static [u8]>) -> usize {
    collect_ids(db, table).map_or(0, |ids| ids.len())
}

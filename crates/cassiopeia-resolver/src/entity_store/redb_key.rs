use crate::{
    entity_store::{
        entity_order::in_source_order,
        error::{EntityStoreError, Result},
        redb_database::RedbDatabase,
    },
    store_action::StoreAction,
};
use cassiopeia_ir::source_position::SourcePosition;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use std::mem;
use urn_rs::Urn;

/// Unit separator delimiting the base id from a key's trailing discriminant (a big-endian mapping id
/// or source position). A URN never contains a control byte, so the first `0x1F` unambiguously ends
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

/// Streams the distinct base ids in `table` chunk by chunk in the order of their first records: the
/// earliest of the positions `position_of` reads from each id's keys and their values.
///
/// The ids are gathered in one scan and sorted as text, which holds each in a fraction of the memory
/// a parsed id takes, and each chunk is parsed as it is handed on.
///
/// # Errors
///
/// Returns [`EntityStoreError`] when the table cannot be read or a base id read back is not a URN,
/// or propagates any error `position_of` or the callback returns.
pub(crate) fn for_each_id_chunk_in_source_order(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    position_of: impl Fn(&[u8], &[u8]) -> Result<SourcePosition>,
    chunk_size: usize,
    callback: &mut dyn FnMut(Vec<Urn>) -> Result<()>,
) -> Result<()> {
    let mut ids = in_source_order(id_firsts(db, table, position_of)?).into_iter();
    loop {
        let chunk = ids
            .by_ref()
            .take(chunk_size)
            .map(|id| id.parse().map_err(|source| EntityStoreError::StoredId { id: String::from(id), source }))
            .collect::<Result<Vec<Urn>>>()?;
        if chunk.is_empty() {
            return Ok(());
        }
        callback(chunk)?;
    }
}

/// Every distinct base id in `table`, in the order of their first records, as
/// [`for_each_id_chunk_in_source_order`] reads them with `position_of`.
///
/// # Errors
///
/// Returns [`EntityStoreError`] when the table cannot be read or a base id read back is not a URN,
/// or propagates any error `position_of` returns.
pub(crate) fn ids_in_source_order(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    position_of: impl Fn(&[u8], &[u8]) -> Result<SourcePosition>,
) -> Result<Vec<Urn>> {
    let mut ids = Vec::new();
    for_each_id_chunk_in_source_order(db, table, position_of, usize::MAX, &mut |chunk| {
        ids.extend(chunk);
        Ok(())
    })?;
    Ok(ids)
}

/// Every distinct base id in `table` as its text, each with the earliest of the positions
/// `position_of` reads from its keys and their values, in the order of the ids' keys.
///
/// Keys sort with the separator ahead of any URN character, so every key of one base id is contiguous
/// and one scan meets each id's keys together.
fn id_firsts(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    position_of: impl Fn(&[u8], &[u8]) -> Result<SourcePosition>,
) -> Result<Vec<(SourcePosition, Box<str>)>> {
    let read_txn = db.database().begin_read().map_err(|e| db.err(e.into(), StoreAction::BeginTransaction))?;
    let table = read_txn.open_table(table).map_err(|e| db.err(e.into(), StoreAction::OpenTable))?;

    let mut firsts: Vec<(SourcePosition, Box<str>)> = Vec::new();
    for entry in table.iter().map_err(|e| db.err(e.into(), StoreAction::Iterate))? {
        let (key, value) = entry.map_err(|e| db.err(e.into(), StoreAction::Iterate))?;
        let key = key.value();
        let position = position_of(key, value.value())?;
        let base = base_id_bytes(key);
        match firsts.last_mut() {
            Some((first, id)) if id.as_bytes() == base => *first = (*first).min(position),
            // A base id that is not UTF-8 keeps a replacement character, so parsing it refuses it.
            _ => firsts.push((position, String::from_utf8_lossy(base).into())),
        }
    }
    Ok(firsts)
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

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        error::EntityStoreError,
        redb_database::RedbDatabase,
        redb_key::{for_each_id_chunk_in_source_order, ids_in_source_order},
    };
    use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
    use pretty_assertions::assert_eq;
    use redb::TableDefinition;
    use urn_rs::Urn;

    const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("keys");

    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    /// A database whose table holds a key for each of `keys`, each the base id followed by its
    /// separator and a discriminant, with the position of its record as its value.
    fn database(keys: &[(&[u8], u8, SourcePosition)]) -> RedbDatabase {
        let db = RedbDatabase::open("keys.redb").unwrap();
        let write_txn = db.database().begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            for (id, discriminant, position) in keys {
                let mut key = id.to_vec();
                key.extend_from_slice(&[0x1F, *discriminant]);
                table.insert(key.as_slice(), position.to_be_bytes().as_slice()).unwrap();
            }
        }
        write_txn.commit().unwrap();
        db
    }

    fn value_position(value: &[u8]) -> SourcePosition {
        SourcePosition::from_be_bytes(<[u8; SOURCE_POSITION_BYTES]>::try_from(value).unwrap())
    }

    #[test]
    fn each_id_comes_once_in_the_order_of_the_earliest_position_among_its_keys() {
        let [short, long] = ["urn:ngsi-ld:Station:S1", "urn:ngsi-ld:Station:S10"].map(|id| id.parse::<Urn>().unwrap());
        let db = database(&[
            (short.as_str().as_bytes(), 2, at(7)),
            (long.as_str().as_bytes(), 1, at(6)),
            (short.as_str().as_bytes(), 1, at(9)),
            (short.as_str().as_bytes(), 3, at(5)),
        ]);

        let mut chunks = Vec::new();
        for_each_id_chunk_in_source_order(&db, TABLE, |_key, value| Ok(value_position(value)), 1, &mut |chunk| {
            chunks.push(chunk);
            Ok(())
        })
        .unwrap();

        assert_eq!(chunks, [vec![short.clone()], vec![long.clone()]]);
        assert_eq!(ids_in_source_order(&db, TABLE, |_key, value| Ok(value_position(value))).unwrap(), [short, long]);
        db.remove_files();
    }

    #[test]
    fn a_base_id_that_is_no_longer_a_urn_is_refused() {
        let db = database(&[(b"not a urn", 1, at(0))]);

        let refused = ids_in_source_order(&db, TABLE, |_key, value| Ok(value_position(value)));

        assert!(matches!(refused, Err(EntityStoreError::StoredId { ref id, .. }) if id == "not a urn"));
        db.remove_files();
    }
}

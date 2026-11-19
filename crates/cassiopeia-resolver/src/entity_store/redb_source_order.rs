use crate::{
    entity_store::{
        error::{EntityStoreError, Result},
        redb_database::RedbDatabase,
        redb_key::base_id_bytes,
    },
    store_action::StoreAction,
};
use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
use extsort::{ExternalSorter, Sortable};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use std::{
    io::{self, Read, Write},
    mem,
};
use urn_rs::Urn;

/// How many ids are sorted in memory at a time. A store with no more entities sorts them in memory
/// alone and writes nothing. A larger one writes each run of this many to a temporary file sorted,
/// one file after the other, and merges the files as it hands the ids on, so it holds one run at a
/// time, a few megabytes, however many entities the run has.
const RUN_IDS: usize = 1 << 16;

/// Streams the distinct base ids in `table` chunk by chunk in the order of their first records: the
/// earliest of the positions `position_of` reads from each id's keys and their values.
///
/// # Errors
///
/// Returns [`EntityStoreError`] when the table cannot be read, the ids cannot be sorted through
/// their temporary files or a base id read back is not a URN, or propagates any error `position_of`
/// or the callback returns.
pub(crate) fn for_each_id_chunk_in_source_order(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    position_of: impl Fn(&[u8], &[u8]) -> Result<SourcePosition>,
    chunk_size: usize,
    callback: &mut dyn FnMut(Vec<Urn>) -> Result<()>,
) -> Result<()> {
    hand_on_in_source_order(db, table, position_of, RUN_IDS, IdChunks::new(chunk_size, callback))
}

/// Every distinct base id in `table`, in the order of their first records, as
/// [`for_each_id_chunk_in_source_order`] reads them with `position_of`.
///
/// # Errors
///
/// Returns [`EntityStoreError`] when the table cannot be read, the ids cannot be sorted through
/// their temporary files or a base id read back is not a URN, or propagates any error `position_of`
/// returns.
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

/// Hands the distinct base ids in `table` on to `chunks` in the order of their first records, sorting
/// them `run_ids` at a time.
///
/// Keys sort with the separator ahead of any URN character, so every key of one base id is contiguous
/// and one scan meets each id's keys together: an id's first record is known once the next id begins.
fn hand_on_in_source_order(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    position_of: impl Fn(&[u8], &[u8]) -> Result<SourcePosition>,
    run_ids: usize,
    mut chunks: IdChunks<'_>,
) -> Result<()> {
    let data_txn = db.database().begin_read().map_err(|e| db.err(e.into(), StoreAction::BeginTransaction))?;
    let data = data_txn.open_table(table).map_err(|e| db.err(e.into(), StoreAction::OpenTable))?;
    let mut sorter = ExternalSorter::new().with_segment_size(run_ids).with_parallel_sort().pushed();
    let mut scanned: Option<FirstRecord> = None;
    for entry in data.iter().map_err(|e| db.err(e.into(), StoreAction::Iterate))? {
        let (key, value) = entry.map_err(|e| db.err(e.into(), StoreAction::Iterate))?;
        let key = key.value();
        let position = position_of(key, value.value())?;
        let base = base_id_bytes(key);
        match &mut scanned {
            Some(first) if first.id == base => first.position = first.position.min(position),
            Some(_) | None => {
                let complete = scanned.replace(FirstRecord { position, id: base.to_vec() });
                sorter.push_iter(complete).map_err(sort_failure)?;
            }
        }
    }
    sorter.push_iter(scanned).map_err(sort_failure)?;

    for first in sorter.done().map_err(sort_failure)? {
        chunks.push(&first.map_err(sort_failure)?.id)?;
    }
    chunks.finish()
}

/// Wraps an I/O failure of the sort's temporary files.
const fn sort_failure(source: io::Error) -> EntityStoreError {
    EntityStoreError::SortIds { source }
}

/// One base id with the position of its first record. Ids order by that position, those sharing one
/// in the bytewise order of their text, so the order is the same on every run.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FirstRecord {
    /// The position of the id's first record.
    position: SourcePosition,
    /// The id's text.
    id: Vec<u8>,
}

/// A run file holds each id as the big-endian bytes of its first position, the length of its text
/// as a big-endian `u32`, then the text. `extsort` defines no encoding of its own, so one is written
/// here.
impl Sortable for FirstRecord {
    fn encode<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let length = u32::try_from(self.id.len()).map_err(io::Error::other)?;
        writer.write_all(&self.position.to_be_bytes())?;
        writer.write_all(&length.to_be_bytes())?;
        writer.write_all(&self.id)
    }

    fn decode<R: Read>(reader: &mut R) -> io::Result<FirstRecord> {
        // The end of a run file surfaces as `UnexpectedEof` on the position, which `extsort` reads as
        // the run being spent.
        let mut position = [0; SOURCE_POSITION_BYTES];
        reader.read_exact(&mut position)?;
        let mut length = [0; 4];
        reader.read_exact(&mut length)?;
        let mut id = vec![0; usize::try_from(u32::from_be_bytes(length)).map_err(io::Error::other)?];
        reader.read_exact(&mut id)?;
        Ok(FirstRecord {
            position: SourcePosition::from_be_bytes(position),
            id,
        })
    }
}

/// Parses ids into chunks of one size and hands each full chunk on.
struct IdChunks<'a> {
    /// How many ids one chunk holds.
    size: usize,
    /// The chunk being filled.
    chunk: Vec<Urn>,
    /// What each chunk is handed to.
    callback: &'a mut dyn FnMut(Vec<Urn>) -> Result<()>,
}

impl<'a> IdChunks<'a> {
    /// Chunks of `size` ids, handed to `callback`.
    fn new(size: usize, callback: &'a mut dyn FnMut(Vec<Urn>) -> Result<()>) -> IdChunks<'a> {
        // The initial capacity is bounded so `ids_in_source_order` can pass `usize::MAX` without a
        // giant reserve.
        IdChunks {
            size,
            chunk: Vec::with_capacity(size.min(1024)),
            callback,
        }
    }

    /// Parses `id` into the chunk, handing the chunk on once it is full.
    fn push(&mut self, id: &[u8]) -> Result<()> {
        // A base id that is not UTF-8 keeps a replacement character, so parsing it refuses it.
        let id = String::from_utf8_lossy(id);
        self.chunk
            .push(id.parse().map_err(|source| EntityStoreError::StoredId { id: id.to_string(), source })?);
        if self.chunk.len() >= self.size {
            (self.callback)(mem::take(&mut self.chunk))?;
        }
        Ok(())
    }

    /// Hands on the last chunk, when it holds any id.
    fn finish(self) -> Result<()> {
        if self.chunk.is_empty() { Ok(()) } else { (self.callback)(self.chunk) }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        error::EntityStoreError,
        redb_database::RedbDatabase,
        redb_source_order::{IdChunks, for_each_id_chunk_in_source_order, hand_on_in_source_order, ids_in_source_order},
    };
    use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
    use pretty_assertions::assert_eq;
    use redb::TableDefinition;
    use urn_rs::Urn;

    const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("keys");

    fn at(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    /// A database whose table holds a key for each of `keys`, each the base id followed by its
    /// separator and a discriminant, with the position of its record as its value.
    fn database(keys: &[(&str, u8, SourcePosition)]) -> RedbDatabase {
        let db = RedbDatabase::open("keys.redb").unwrap();
        let write_txn = db.database().begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            for (id, discriminant, position) in keys {
                let mut key = id.as_bytes().to_vec();
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

    /// Every id `db` hands on, sorted `run_ids` at a time.
    fn handed_on(db: &RedbDatabase, run_ids: usize) -> Vec<Urn> {
        let mut ids = Vec::new();
        let mut collect = |chunk: Vec<Urn>| {
            ids.extend(chunk);
            Ok(())
        };
        hand_on_in_source_order(db, TABLE, |_key, value| Ok(value_position(value)), run_ids, IdChunks::new(2, &mut collect)).unwrap();
        ids
    }

    #[test]
    fn each_id_comes_once_in_the_order_of_the_earliest_position_among_its_keys() {
        let [short, long] = ["urn:ngsi-ld:Station:S1", "urn:ngsi-ld:Station:S10"].map(urn);
        let db = database(&[
            (short.as_str(), 2, at(7)),
            (long.as_str(), 1, at(6)),
            (short.as_str(), 1, at(9)),
            (short.as_str(), 3, at(5)),
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
    fn ids_sorted_in_runs_written_to_disk_come_in_the_order_of_their_first_records_once_on_every_hand_off() {
        // Five ids whose keys sort in another order than their first records, two of them sharing one.
        let db = database(&[
            ("urn:ngsi-ld:Station:A", 1, at(4)),
            ("urn:ngsi-ld:Station:B", 1, at(0)),
            ("urn:ngsi-ld:Station:B", 2, at(8)),
            ("urn:ngsi-ld:Station:C", 1, at(9)),
            ("urn:ngsi-ld:Station:C", 2, at(2)),
            ("urn:ngsi-ld:Station:D", 1, at(4)),
            ("urn:ngsi-ld:Station:E", 1, at(1)),
        ]);
        let expected = ["B", "E", "C", "A", "D"].map(|name| urn(&format!("urn:ngsi-ld:Station:{name}")));

        // Runs of two write every run to disk, and a second hand-off sorts afresh.
        assert_eq!(handed_on(&db, 2), expected);
        assert_eq!(handed_on(&db, 2), expected);
        // A run of every id writes nothing and hands on the same.
        assert_eq!(handed_on(&db, 5), expected);
        db.remove_files();
    }

    #[test]
    fn a_base_id_that_is_no_longer_a_urn_is_refused() {
        let db = database(&[("not a urn", 1, at(0))]);

        let refused = ids_in_source_order(&db, TABLE, |_key, value| Ok(value_position(value)));

        assert!(matches!(refused, Err(EntityStoreError::StoredId { ref id, .. }) if id == "not a urn"));
        db.remove_files();
    }
}

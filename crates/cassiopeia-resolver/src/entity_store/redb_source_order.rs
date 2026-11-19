use crate::{
    entity_store::{
        error::{EntityStoreError, Result},
        redb_database::RedbDatabase,
        redb_key::base_id_bytes,
    },
    store_action::StoreAction,
};
use cassiopeia_ir::source_position::{SOURCE_POSITION_BYTES, SourcePosition};
use rayon::slice::ParallelSliceMut;
use redb::{Durability, ReadableDatabase, ReadableTable, Table, TableDefinition};
use std::{mem, ops::Range};
use urn_rs::Urn;

/// The table the ids of a store too large to sort in one run are sorted through, in the store's own
/// database. Each key is the big-endian position of an id's first record followed by the id's text,
/// and holds nothing, so the table lists the ids in source order, those sharing a first record in the
/// bytewise order of their text.
const ORDER_TABLE: TableDefinition<&[u8], ()> = TableDefinition::new("order");

/// How many ids are sorted in memory at a time. A store with no more entities sorts them in memory
/// alone and writes nothing. A larger one writes each run of this many to [`ORDER_TABLE`] sorted, so
/// each insertion lands beside the one before it, and holds one run at a time, about a megabyte,
/// however many entities the run has.
const RUN_IDS: usize = 1 << 14;

/// Streams the distinct base ids in `table` chunk by chunk in the order of their first records: the
/// earliest of the positions `position_of` reads from each id's keys and their values.
///
/// # Errors
///
/// Returns [`EntityStoreError`] when a table cannot be read or written or a base id read back is not
/// a URN, or propagates any error `position_of` or the callback returns.
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
/// Returns [`EntityStoreError`] when a table cannot be read or written or a base id read back is not
/// a URN, or propagates any error `position_of` returns.
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
/// and one scan meets each id's keys together: an id is complete once the next one begins, so a run
/// never splits an id.
fn hand_on_in_source_order(
    db: &RedbDatabase,
    table: TableDefinition<'static, &'static [u8], &'static [u8]>,
    position_of: impl Fn(&[u8], &[u8]) -> Result<SourcePosition>,
    run_ids: usize,
    mut chunks: IdChunks<'_>,
) -> Result<()> {
    let data_txn = db.database().begin_read().map_err(|e| db.err(e.into(), StoreAction::BeginTransaction))?;
    let data = data_txn.open_table(table).map_err(|e| db.err(e.into(), StoreAction::OpenTable))?;
    let mut write_txn = db.database().begin_write().map_err(|e| db.err(e.into(), StoreAction::BeginTransaction))?;
    // The table lives only as long as the run's temporary database, so fsync would be pure waste.
    write_txn
        .set_durability(Durability::None)
        .map_err(|e| db.err(e.into(), StoreAction::SetDurability))?;
    // The ids an earlier hand-off sorted would interleave with this one's.
    write_txn.delete_table(ORDER_TABLE).map_err(|e| db.err(e.into(), StoreAction::DeleteTable))?;

    let mut run = SortRun::default();
    let mut spilled = Spilled::Nothing;
    {
        let mut order = write_txn.open_table(ORDER_TABLE).map_err(|e| db.err(e.into(), StoreAction::OpenTable))?;
        for entry in data.iter().map_err(|e| db.err(e.into(), StoreAction::Iterate))? {
            let (key, value) = entry.map_err(|e| db.err(e.into(), StoreAction::Iterate))?;
            let key = key.value();
            let position = position_of(key, value.value())?;
            let base = base_id_bytes(key);
            if let Some(first) = run.first_of_last(base) {
                *first = (*first).min(position);
                continue;
            }
            if run.len() == run_ids {
                run.spill(&mut order, db)?;
                spilled = Spilled::Runs;
            }
            run.push(position, base);
        }
        if spilled == Spilled::Runs {
            run.spill(&mut order, db)?;
        }
    }

    match spilled {
        Spilled::Nothing => {
            // Every id fit one run, so the ids are handed on from memory and nothing is written.
            write_txn.abort().map_err(|e| db.err(e.into(), StoreAction::AbortTransaction))?;
            run.sorted().try_for_each(|(_first, id)| chunks.push(id))?;
        }
        Spilled::Runs => {
            write_txn.commit().map_err(|e| db.err(e.into(), StoreAction::CommitTransaction))?;
            let order_txn = db.database().begin_read().map_err(|e| db.err(e.into(), StoreAction::BeginTransaction))?;
            let order = order_txn.open_table(ORDER_TABLE).map_err(|e| db.err(e.into(), StoreAction::OpenTable))?;
            for entry in order.iter().map_err(|e| db.err(e.into(), StoreAction::Iterate))? {
                let (key, _nothing) = entry.map_err(|e| db.err(e.into(), StoreAction::Iterate))?;
                let key = key.value();
                let (_first, id) = key
                    .split_at_checked(SOURCE_POSITION_BYTES)
                    .ok_or(EntityStoreError::TruncatedPosition { length: key.len() })?;
                chunks.push(id)?;
            }
        }
    }
    chunks.finish()
}

/// Whether a hand-off wrote any run of its ids to [`ORDER_TABLE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spilled {
    /// Every id fit one run, held in memory.
    Nothing,
    /// At least one run was written, so every run is.
    Runs,
}

/// The ids of one run, each with the position of its first record, their text kept in one buffer
/// rather than in an allocation each.
#[derive(Default)]
struct SortRun {
    /// Each id's first position, with the place of its text in `text`, in the order the scan met them.
    firsts: Vec<(SourcePosition, Range<usize>)>,
    /// The text of every id of the run, one after the other.
    text: Vec<u8>,
}

impl SortRun {
    /// How many ids the run holds.
    const fn len(&self) -> usize {
        self.firsts.len()
    }

    /// The first position of the id the run took last, when that id is `id`.
    fn first_of_last(&mut self, id: &[u8]) -> Option<&mut SourcePosition> {
        let text = &self.text;
        self.firsts
            .last_mut()
            .filter(|(_first, range)| text.get(range.clone()) == Some(id))
            .map(|(first, _range)| first)
    }

    /// Takes `id`, whose first record so far is at `first`.
    fn push(&mut self, first: SourcePosition, id: &[u8]) {
        let start = self.text.len();
        self.text.extend_from_slice(id);
        self.firsts.push((first, start..self.text.len()));
    }

    /// The run's ids, each with its first position, sorted by that position, those sharing one in the
    /// bytewise order of their text.
    fn sorted(&mut self) -> impl Iterator<Item = (SourcePosition, &[u8])> {
        let text = &self.text;
        self.firsts.par_sort_unstable_by(|(left_first, left), (right_first, right)| {
            left_first.cmp(right_first).then_with(|| text.get(left.clone()).cmp(&text.get(right.clone())))
        });
        self.firsts.iter().filter_map(|(first, range)| text.get(range.clone()).map(|id| (*first, id)))
    }

    /// Writes the run's ids into `order` sorted and empties the run for the next one.
    fn spill(&mut self, order: &mut Table<'_, &'static [u8], ()>, db: &RedbDatabase) -> Result<()> {
        let mut key = Vec::new();
        for (first, id) in self.sorted() {
            key.clear();
            key.extend_from_slice(&first.to_be_bytes());
            key.extend_from_slice(id);
            order.insert(key.as_slice(), ()).map_err(|e| db.err(e.into(), StoreAction::Insert))?;
        }
        self.firsts.clear();
        self.text.clear();
        Ok(())
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

        // Runs of two write every run to disk; a second hand-off finds the first one's ids gone.
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

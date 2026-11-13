use crate::{cursor_store_error::CursorStoreError, position::Position, positions::Positions};
use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId};
use redb::{ReadableTable, Table, TableDefinition};
use std::collections::BTreeMap;

/// The table of a store's positions: each data stream id, encoded as `MessagePack`, maps to its
/// position, encoded as `MessagePack` with named fields, so a later format can read what an earlier
/// one wrote.
pub(crate) const POSITIONS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("positions");

/// Every position `table`, the positions table of the cursor `cursor`, holds.
///
/// # Errors
///
/// Returns [`CursorStoreError::UnreadablePosition`] when an id or a position cannot be decoded, and
/// [`CursorStoreError::Storage`] when the table cannot be read.
pub(crate) fn read_positions(table: &impl ReadableTable<&'static [u8], &'static [u8]>, cursor: &CursorName) -> Result<Positions, CursorStoreError> {
    let unreadable = |source| CursorStoreError::UnreadablePosition {
        // Each error owns the name of the cursor it names, and outlives the store.
        cursor: cursor.clone(),
        source,
    };
    table
        .iter()
        .map_err(CursorStoreError::storage(cursor))?
        .map(|entry| {
            let (key, value) = entry.map_err(CursorStoreError::storage(cursor))?;
            let datastream: EntityId = rmp_serde::from_slice(key.value()).map_err(unreadable)?;
            let position: Position = rmp_serde::from_slice(value.value()).map_err(unreadable)?;
            Ok((datastream, position))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map(Positions::from)
}

/// Replaces every position `table`, the positions table of the cursor `cursor`, holds with
/// `positions`.
///
/// # Errors
///
/// Returns [`CursorStoreError::UnwritablePosition`] when an id or a position cannot be encoded, and
/// [`CursorStoreError::Storage`] when the table cannot be written.
pub(crate) fn write_positions(table: &mut Table<'_, &'static [u8], &'static [u8]>, positions: Positions, cursor: &CursorName) -> Result<(), CursorStoreError> {
    let unwritable = |source| CursorStoreError::UnwritablePosition {
        // Each error owns the name of the cursor it names, and outlives the store.
        cursor: cursor.clone(),
        source,
    };
    table.retain(|_, _| false).map_err(CursorStoreError::storage(cursor))?;
    for (datastream, position) in BTreeMap::from(positions) {
        let key = rmp_serde::to_vec(&datastream).map_err(unwritable)?;
        let value = rmp_serde::to_vec_named(&position).map_err(unwritable)?;
        table.insert(key.as_slice(), value.as_slice()).map_err(CursorStoreError::storage(cursor))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        cursor_update::CursorUpdate,
        positions::Positions,
        positions_table::{POSITIONS, read_positions, write_positions},
        seen_observations::SeenObservations,
    };
    use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId};
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use redb::{Database, ReadableDatabase};
    use std::{collections::BTreeMap, time::Duration};

    #[test]
    fn integer_and_text_ids_and_their_positions_are_read_back_as_written() {
        let folder = tempfile::tempdir().unwrap();
        let database = Database::create(folder.path().join("positions.redb")).unwrap();
        let cursor = CursorName::try_from("rivm-air".to_owned()).unwrap();
        let time = DateTime::parse_from_rfc3339("2026-11-12T09:30:00.250Z").unwrap().to_utc();
        let mut update = CursorUpdate::new(time, Duration::from_secs(3600));
        update.record(EntityId::Integer(5), SeenObservations::from(BTreeMap::from([(EntityId::Integer(70), time)])));
        update.record(
            EntityId::Text("t-5".into()),
            SeenObservations::from(BTreeMap::from([(EntityId::Text("o-1".into()), time)])),
        );
        let positions = update.apply(Positions::default());

        let transaction = database.begin_write().unwrap();
        {
            let mut table = transaction.open_table(POSITIONS).unwrap();
            write_positions(&mut table, positions.clone(), &cursor).unwrap();
        }
        transaction.commit().unwrap();
        let read = read_positions(&database.begin_read().unwrap().open_table(POSITIONS).unwrap(), &cursor).unwrap();

        assert_eq!(read, positions);
        assert_eq!(read.len(), 2);
    }
}

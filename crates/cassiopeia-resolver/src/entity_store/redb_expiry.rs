use crate::{
    entity_store::{error::Result, redb_database::RedbDatabase},
    store_action::StoreAction,
};
use ahash::AHashMap;
use chrono::{DateTime, Utc};
use redb::{ReadableTable, Table, TableDefinition};
use std::collections::hash_map::Entry;
use urn_rs::Urn;

/// The table holding each base id's merged entity `expiresAt`, keyed by the id's UTF-8 bytes.
pub(crate) const EXPIRIES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("expiries");

/// The entity expiries one write batch contributes, merged per base id before they reach disk.
///
/// Every fragment of an id may declare its own expiry, and clause 4.5.1 of ETSI GS CIM 009 v1.9.1
/// gives `expiresAt` one instant per entity, so the greatest wins. Folding a batch in memory first
/// means each id's row is read and rewritten at most once per batch, however many of its fragments
/// the batch holds.
#[derive(Default)]
pub(crate) struct PendingExpiries(AHashMap<Vec<u8>, DateTime<Utc>>);

impl PendingExpiries {
    /// Merges one fragment's expiry into the pending instant for its base id.
    pub(crate) fn add(&mut self, base_id: &Urn, expires_at: DateTime<Utc>) {
        match self.0.entry(base_id.as_str().as_bytes().to_vec()) {
            Entry::Occupied(mut pending) => {
                if expires_at > *pending.get() {
                    *pending.get_mut() = expires_at;
                }
            }
            Entry::Vacant(vacant) => {
                vacant.insert(expires_at);
            }
        }
    }

    /// Merges each pending instant into the one already on disk and writes back only the rows that
    /// moved forwards.
    ///
    /// The stored instant is merged into rather than replaced, so an expiry an earlier batch recorded
    /// survives a later batch whose fragments declare none or earlier ones.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when a row cannot
    /// be read, decoded, encoded, or written.
    pub(crate) fn flush(self, table: &mut Table<'_, &'static [u8], &'static [u8]>, db: &RedbDatabase) -> Result<()> {
        for (key, incoming) in self.0 {
            let stored: Option<DateTime<Utc>> = match table.get(key.as_slice()).map_err(|e| db.err(e.into(), StoreAction::Read))? {
                Some(guard) => Some(rmp_serde::from_slice(guard.value())?),
                None => None,
            };
            if stored.is_some_and(|stored| stored >= incoming) {
                continue;
            }
            let encoded = rmp_serde::to_vec(&incoming)?;
            table
                .insert(key.as_slice(), encoded.as_slice())
                .map_err(|e| db.err(e.into(), StoreAction::Insert))?;
        }
        Ok(())
    }
}

/// Reads the merged expiry stored for `base_id`, or `None` when none of its fragments had one.
///
/// # Errors
///
/// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the row cannot be
/// read or decoded.
pub(crate) fn read_expiry(table: &impl ReadableTable<&'static [u8], &'static [u8]>, base_id: &Urn, db: &RedbDatabase) -> Result<Option<DateTime<Utc>>> {
    match table.get(base_id.as_str().as_bytes()).map_err(|e| db.err(e.into(), StoreAction::Read))? {
        Some(guard) => Ok(Some(rmp_serde::from_slice(guard.value())?)),
        None => Ok(None),
    }
}

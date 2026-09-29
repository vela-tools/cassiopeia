use crate::{
    entity_store::{error::Result, redb_database::RedbDatabase},
    store_action::StoreAction,
};
use ahash::AHashMap;
use cassiopeia_ngsi_ld::entity::scope::{NgsiLdScope, ScopeMerge};
use redb::{ReadableTable, Table, TableDefinition};
use std::collections::hash_map::Entry;
use urn_rs::Urn;

/// The table holding each base id's merged scope set, keyed by the id's UTF-8 bytes.
pub(crate) const SCOPES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("scopes");

/// The scope sets one write batch contributes, merged per base id before they reach disk.
///
/// Every fragment of an id may declare its own scopes, and clause 4.18 of ETSI GS CIM 009 v1.9.1
/// merges them when representations of one entity are combined. Folding a batch in memory first
/// means each id's row is read and rewritten at most once per batch, however many of its fragments
/// the batch holds.
#[derive(Default)]
pub(crate) struct PendingScopes(AHashMap<Vec<u8>, NgsiLdScope>);

impl PendingScopes {
    /// Merges one fragment's scopes into the pending set for its base id.
    pub(crate) fn add(&mut self, base_id: &Urn, scope: NgsiLdScope) {
        match self.0.entry(base_id.as_str().as_bytes().to_vec()) {
            Entry::Occupied(mut pending) => {
                pending.get_mut().merge(scope);
            }
            Entry::Vacant(vacant) => {
                vacant.insert(scope);
            }
        }
    }

    /// Merges each pending set into the set already on disk and writes back only the rows that grew.
    ///
    /// The stored set is merged into rather than replaced, so scopes an earlier batch recorded survive
    /// a later batch that declares different ones.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when a row cannot be
    /// read, decoded, encoded, or written.
    pub(crate) fn flush(self, table: &mut Table<'_, &'static [u8], &'static [u8]>, db: &RedbDatabase) -> Result<()> {
        for (key, incoming) in self.0 {
            let stored: Option<NgsiLdScope> = match table.get(key.as_slice()).map_err(|e| db.err(e.into(), StoreAction::Read))? {
                Some(guard) => Some(rmp_serde::from_slice(guard.value())?),
                None => None,
            };
            let merged = match stored {
                Some(mut stored) => match stored.merge(incoming) {
                    ScopeMerge::Unchanged => continue,
                    ScopeMerge::Extended => stored,
                },
                None => incoming,
            };
            let encoded = rmp_serde::to_vec(&merged)?;
            table
                .insert(key.as_slice(), encoded.as_slice())
                .map_err(|e| db.err(e.into(), StoreAction::Insert))?;
        }
        Ok(())
    }
}

/// Reads the merged scope set stored for `base_id`, or `None` when none of its fragments had one.
///
/// # Errors
///
/// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the row cannot be
/// read or decoded.
pub(crate) fn read_scope(table: &impl ReadableTable<&'static [u8], &'static [u8]>, base_id: &Urn, db: &RedbDatabase) -> Result<Option<NgsiLdScope>> {
    match table.get(base_id.as_str().as_bytes()).map_err(|e| db.err(e.into(), StoreAction::Read))? {
        Some(guard) => Ok(Some(rmp_serde::from_slice(guard.value())?)),
        None => Ok(None),
    }
}

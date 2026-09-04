use crate::{relationship_store::error::Result, store_write_strategy::StoreWriteStrategy};
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use std::fmt::Debug;
use urn_rs::Urn;

/// One parent's relationships, grouped by the property path they were stored under.
///
/// The keys are attribute names a mapping declared, so the grouping hashes with `foldhash` rather
/// than the standard library's `SipHash`: nothing untrusted reaches these keys, and the map is rebuilt
/// for every assembled entity. It stays an [`IndexMap`] because assembly relies on the properties
/// coming back in insertion order.
pub type StoredRelationships = IndexMap<String, Vec<Urn>, RandomState>;

/// Stores parent-child relationships discovered during resolution.
///
/// Relationships are written via [`add_child`](Self::add_child) and consumed via either
/// [`take_all_relationships`](Self::take_all_relationships), which removes and returns them, or
/// [`get_all_relationships`](Self::get_all_relationships), which reads them without mutating the
/// store.
pub trait RelationshipStore: Debug + Send + Sync + 'static {
    /// The write pattern this backend is optimized for.
    fn write_strategy(&self) -> StoreWriteStrategy;

    /// Records a parent-child relationship under the given property name.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store write fails.
    fn add_child(&self, parent_urn: &Urn, property: &str, child_urn: &Urn) -> Result<()>;

    /// Records many parent-child relationships in a single call.
    ///
    /// Disk-backed implementations coalesce the writes into one transaction
    /// rather than issuing one I/O per relationship. The default implementation
    /// falls back to [`add_child`](Self::add_child) per entry.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store write fails.
    fn add_child_batch(&self, entries: &[(Urn, &str, Urn)]) -> Result<()> {
        for (parent, property, child) in entries {
            self.add_child(parent, property, child)?;
        }
        Ok(())
    }

    /// Removes and returns all relationships for the given parent URN.
    ///
    /// Returns a [`StoredRelationships`] map from property name to the child URN list, empty
    /// when the parent has none.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store cannot be read or updated.
    fn take_all_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships>;

    /// Read-only counterpart of [`take_all_relationships`](Self::take_all_relationships).
    ///
    /// Returns the same grouped map but leaves the store untouched. Entity assembly visits each
    /// URN once and then calls [`destroy`](Self::destroy), so skipping the remove halves the disk
    /// work; for redb it also lets
    /// callers open concurrent read transactions instead of serializing on the
    /// single writer. The default implementation delegates to the removing
    /// variant; concrete backends override it with a read-only path.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store cannot be read.
    fn get_all_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        self.take_all_relationships(parent_urn)
    }

    /// Releases all stored relationships.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store's resources cannot be released.
    fn destroy(&self) -> Result<()>;

    /// Creates a boxed clone of this store.
    fn clone_box(&self) -> Box<dyn RelationshipStore>;
}

impl Clone for Box<dyn RelationshipStore> {
    fn clone(&self) -> Box<dyn RelationshipStore> {
        self.clone_box()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::relationship_store::store::RelationshipStore;
    use urn_rs::Urn;

    /// Cross-backend contract every [`RelationshipStore`] must honour: a child
    /// list comes back in exact insertion order (never sorted by child URN) and
    /// duplicate edges are retained. Both the in-memory and disk backends call
    /// this so they are held to identical behaviour.
    pub(crate) fn assert_preserves_insertion_order_and_duplicates(store: &dyn RelationshipStore) {
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child_c: Urn = "urn:ngsi-ld:Child:C".parse().unwrap();
        let child_a: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();
        let child_b: Urn = "urn:ngsi-ld:Child:B".parse().unwrap();
        let dup: Urn = "urn:ngsi-ld:Child:X".parse().unwrap();

        // Reverse-lexicographic insertion: a sort-by-child backend would return
        // [A, B, C] instead of the [C, A, B] inserted here.
        store.add_child(&parent, "hasPart", &child_c).unwrap();
        store.add_child(&parent, "hasPart", &child_a).unwrap();
        store.add_child(&parent, "hasPart", &child_b).unwrap();
        // Same edge inserted twice: both instances must survive.
        store.add_child(&parent, "hasDup", &dup).unwrap();
        store.add_child(&parent, "hasDup", &dup).unwrap();

        let rels = store.take_all_relationships(&parent).unwrap();
        assert_eq!(rels.get("hasPart").map(Vec::as_slice), Some(&[child_c, child_a, child_b][..]));
        assert_eq!(rels.get("hasDup").map(Vec::as_slice), Some(&[dup.clone(), dup][..]));
    }
}

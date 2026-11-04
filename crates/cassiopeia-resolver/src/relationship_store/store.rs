use crate::{relationship_store::error::Result, store_write_strategy::StoreWriteStrategy};
use foldhash::fast::RandomState;
use indexmap::{IndexMap, IndexSet};
use std::fmt::Debug;
use urn_rs::Urn;

/// One parent's relationships, grouped by the relationship key they were stored under.
///
/// Each key is the text form of a
/// [`RelationshipKey`](cassiopeia_ir::relationship_key::RelationshipKey): a relationship path, or an
/// attribute name and an instance index. The keys are built from attribute names a mapping declared,
/// so the grouping hashes with `foldhash` rather than the standard library's `SipHash`: nothing
/// untrusted reaches these keys, and the map is rebuilt for every assembled entity. It stays an
/// [`IndexMap`] because assembly relies on the keys coming back in insertion order.
///
/// Each key's targets are a set in first-insertion order: when several records resolve to one
/// entity, each repeats the edges it shares with the others, and the entity links each target once,
/// at the position it first appeared. The targets are URNs read from source data, so the set keeps
/// the standard library's seeded hasher.
pub type StoredRelationships = IndexMap<String, IndexSet<Urn>, RandomState>;

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
    /// Returns a [`StoredRelationships`] map from property name to the distinct child URNs in the
    /// order each was first added, empty when the parent has none.
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
    use crate::relationship_store::store::{RelationshipStore, StoredRelationships};
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    /// The children stored under `property`, in the order the set holds them: an `IndexSet`
    /// compares equal to any set of the same members, so order is asserted on the sequence.
    pub(crate) fn in_order<'a>(relationships: &'a StoredRelationships, property: &str) -> Vec<&'a Urn> {
        relationships.get(property).into_iter().flatten().collect()
    }

    /// Cross-backend contract every [`RelationshipStore`] must honour: a child
    /// list comes back in exact first-insertion order (never sorted by child URN)
    /// and a repeated edge comes back once, at the position it was first added.
    /// Both the in-memory and disk backends call this so they are held to
    /// identical behaviour.
    pub(crate) fn assert_keeps_first_insertion_order_without_repeats(store: &dyn RelationshipStore) {
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child_c: Urn = "urn:ngsi-ld:Child:C".parse().unwrap();
        let child_a: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();
        let child_b: Urn = "urn:ngsi-ld:Child:B".parse().unwrap();
        let child_d: Urn = "urn:ngsi-ld:Child:D".parse().unwrap();

        // Reverse-lexicographic insertion: a sort-by-child backend would return
        // [A, B, C] instead of the [C, A, B] inserted here.
        store.add_child(&parent, "hasPart", &child_c).unwrap();
        store.add_child(&parent, "hasPart", &child_a).unwrap();
        store.add_child(&parent, "hasPart", &child_b).unwrap();
        // Three records repeating one list, the last adding a new child: the
        // repeats collapse onto their first positions and the new child follows.
        for _ in 0..3 {
            for child in [&child_c, &child_a, &child_b] {
                store.add_child(&parent, "hasRepeat", child).unwrap();
            }
        }
        store.add_child(&parent, "hasRepeat", &child_d).unwrap();
        store.add_child(&parent, "hasRepeat", &child_a).unwrap();

        let expected_parts = [&child_c, &child_a, &child_b];
        let expected_repeats = [&child_c, &child_a, &child_b, &child_d];

        let read = store.get_all_relationships(&parent).unwrap();
        assert_eq!(in_order(&read, "hasPart"), expected_parts);
        assert_eq!(in_order(&read, "hasRepeat"), expected_repeats);

        let taken = store.take_all_relationships(&parent).unwrap();
        assert_eq!(in_order(&taken, "hasPart"), expected_parts);
        assert_eq!(in_order(&taken, "hasRepeat"), expected_repeats);
    }
}

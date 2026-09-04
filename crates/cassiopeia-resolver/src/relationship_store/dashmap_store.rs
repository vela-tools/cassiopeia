use crate::{
    relationship_store::{
        error::Result,
        store::{RelationshipStore, StoredRelationships},
    },
    store_write_strategy::StoreWriteStrategy,
};
use ahash::RandomState;
use dashmap::DashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use tracing::info;
use urn_rs::Urn;

/// Per-parent relationship bucket. Most entities carry one or two distinct
/// relationship properties, so an inline `SmallVec` of 4 avoids a heap
/// allocation in the common case.
type ParentEntries = SmallVec<[(Arc<str>, Urn); 4]>;

/// In-memory relationship store using [`DashMap`] for lock-free concurrent access.
///
/// One outer map keyed directly by the base parent [`Urn`] maps to a `SmallVec` of
/// `(property, child_urn)` pairs. Every observation of one id shares the base id, so a temporal
/// mapping's repeated static relationship accumulates under the one key; the resolver deduplicates
/// those repeats before they reach the store.
///
/// Property names are interned through a separate `DashMap<String, Arc<str>>`:
/// typical datasets have a handful of distinct relationship property names, so
/// every `(_, property, _)` reuses the same heap allocation.
#[derive(Debug, Clone)]
pub struct DashMapRelationshipStore {
    relationships: Arc<DashMap<Urn, ParentEntries, RandomState>>,
    property_interner: Arc<DashMap<String, Arc<str>, RandomState>>,
}

impl DashMapRelationshipStore {
    /// Creates a new, empty in-memory relationship store.
    pub fn new() -> DashMapRelationshipStore {
        info!("Relationship store created (DashMap)");
        DashMapRelationshipStore {
            relationships: Arc::new(DashMap::with_hasher(RandomState::new())),
            property_interner: Arc::new(DashMap::with_hasher(RandomState::new())),
        }
    }

    fn intern_property(&self, property: &str) -> Arc<str> {
        if let Some(existing) = self.property_interner.get(property) {
            return existing.clone();
        }
        let arc: Arc<str> = Arc::from(property);
        self.property_interner.entry(property.to_string()).or_insert_with(|| arc.clone()).clone()
    }
}

impl RelationshipStore for DashMapRelationshipStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::Concurrent
    }

    fn add_child(&self, parent_urn: &Urn, property: &str, child_urn: &Urn) -> Result<()> {
        let property = self.intern_property(property);
        self.relationships.entry(parent_urn.clone()).or_default().push((property, child_urn.clone()));
        Ok(())
    }

    fn take_all_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        let entries = self.relationships.remove(parent_urn).map(|(_, entries)| entries).unwrap_or_default();

        let mut grouped = StoredRelationships::default();
        for (property, child) in entries {
            grouped.entry(property.as_ref().to_string()).or_default().push(child);
        }
        Ok(grouped)
    }

    fn get_all_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        let mut grouped = StoredRelationships::default();
        if let Some(entries) = self.relationships.get(parent_urn) {
            for (property, child) in entries.iter() {
                grouped.entry(property.as_ref().to_string()).or_default().push(child.clone());
            }
        }
        Ok(grouped)
    }

    fn destroy(&self) -> Result<()> {
        self.relationships.clear();
        self.property_interner.clear();
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn RelationshipStore> {
        Box::new(self.clone())
    }
}

impl Default for DashMapRelationshipStore {
    fn default() -> DashMapRelationshipStore {
        DashMapRelationshipStore::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::relationship_store::{
        dashmap_store::DashMapRelationshipStore,
        store::{RelationshipStore, tests::assert_preserves_insertion_order_and_duplicates},
    };
    use urn_rs::Urn;

    #[test]
    fn honours_cross_backend_order_and_duplicate_contract() {
        let store = DashMapRelationshipStore::new();
        assert_preserves_insertion_order_and_duplicates(&store);
    }

    #[test]
    fn the_grouped_properties_come_back_in_first_write_order() {
        // The grouping is an `IndexMap`, so its key order is the order the properties were first
        // written and is independent of the hasher backing it. Reverse-lexicographic writes make a
        // hash- or sort-ordered map return the opposite sequence.
        let store = DashMapRelationshipStore::new();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();

        for property in ["zone", "hasPart", "operatedBy"] {
            store.add_child(&parent, property, &child).unwrap();
        }

        let grouped = store.get_all_relationships(&parent).unwrap();
        assert_eq!(grouped.keys().map(String::as_str).collect::<Vec<&str>>(), ["zone", "hasPart", "operatedBy"]);
    }

    #[test]
    fn get_all_is_non_destructive_while_take_drains() {
        let store = DashMapRelationshipStore::new();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let child: Urn = "urn:ngsi-ld:Child:A".parse().unwrap();

        store.add_child(&parent, "hasPart", &child).unwrap();

        let read = store.get_all_relationships(&parent).unwrap();
        assert_eq!(read.get("hasPart").map(Vec::as_slice), Some(&[child.clone()][..]));

        let taken = store.take_all_relationships(&parent).unwrap();
        assert_eq!(taken.get("hasPart").map(Vec::as_slice), Some(&[child][..]));

        let after = store.take_all_relationships(&parent).unwrap();
        assert!(after.is_empty());
    }
}

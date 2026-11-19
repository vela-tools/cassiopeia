use crate::{
    relationship_store::{
        error::Result,
        relationship_edge::FragmentEdges,
        store::{RelationshipStore, StoredRelationships, join},
    },
    store_write_strategy::StoreWriteStrategy,
};
use ahash::RandomState;
use cassiopeia_ir::source_position::SourcePosition;
use dashmap::DashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use tracing::info;
use urn_rs::Urn;

/// One target a parent was given, beside the key it was given under and the place in the source of
/// the fragment that gave it.
///
/// The key is interned behind a thin pointer, an `Arc<String>` rather than the twice as wide
/// `Arc<str>`, so a target with its position takes 64 bytes, 8 more than one without: a parent's
/// targets grow by half the size of their positions, and the spare capacity of a bucket grows no
/// more than that.
#[derive(Debug, Clone)]
struct StoredTarget {
    /// Where the fragment that gave the target stands in the run's sources.
    position: SourcePosition,
    /// The relationship key the target was given under, interned.
    property: Arc<String>,
    /// The target.
    child: Urn,
}

/// Per-parent relationship bucket. Most entities carry one to three targets, so an inline `SmallVec`
/// avoids a heap allocation in the common case. Three targets beside their positions take less room
/// than four took without them, so the bucket, and every slot of the map holding it, is smaller than
/// before positions were kept.
type ParentEntries = SmallVec<[StoredTarget; 3]>;

/// In-memory relationship store using [`DashMap`] for lock-free concurrent access.
///
/// One outer map keyed directly by the base parent [`Urn`] maps to a `SmallVec` of the targets the
/// parent was given, each beside its key and its fragment's position. The fragments of one batch are
/// written concurrently, so the targets are kept in the order the writes landed and put in the order
/// of the source when the parent's relationships are read. Every observation of one id shares the
/// base id, so a temporal mapping's repeated static relationship accumulates under the one key; the
/// resolver keeps the earliest of those repeats before they reach the store. Every other repeated
/// edge is kept as written and collapses onto its earliest place when the parent's relationships are
/// read.
///
/// Property names are interned through a separate `DashMap<String, Arc<String>>`:
/// typical datasets have a handful of distinct relationship property names, so
/// every stored target reuses the same heap allocation for its key.
#[derive(Debug, Clone)]
pub struct DashMapRelationshipStore {
    relationships: Arc<DashMap<Urn, ParentEntries, RandomState>>,
    property_interner: Arc<DashMap<String, Arc<String>, RandomState>>,
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

    fn intern_property(&self, property: &str) -> Arc<String> {
        if let Some(existing) = self.property_interner.get(property) {
            return Arc::clone(&existing);
        }
        let interned = self
            .property_interner
            .entry(property.to_owned())
            .or_insert_with(|| Arc::new(property.to_owned()));
        Arc::clone(&interned)
    }
}

/// Joins `entries` into one parent's relationships in the order of the source: by position, and one
/// fragment's targets, which share its position, in the order they were written, since the sort is
/// stable.
fn joined_in_source_order(mut entries: ParentEntries) -> StoredRelationships {
    entries.sort_by_key(|stored| stored.position);
    let mut relationships = StoredRelationships::default();
    for stored in entries {
        join(&mut relationships, &stored.property, stored.child);
    }
    relationships
}

impl RelationshipStore for DashMapRelationshipStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::Concurrent
    }

    fn add_edges(&self, edges: FragmentEdges<'_>) -> Result<()> {
        // A fragment's edges all name one parent in practice, so each run of edges sharing a parent
        // takes its bucket once.
        for run in edges.edges.chunk_by(|left, right| left.parent == right.parent) {
            let Some(first) = run.first() else {
                continue;
            };
            // The targets are built before the bucket is locked, so interning never runs under the
            // lock of a shard.
            let stored: ParentEntries = run
                .iter()
                .map(|edge| StoredTarget {
                    position: edges.position,
                    property: self.intern_property(&edge.property),
                    child: edge.child.clone(),
                })
                .collect();
            match self.relationships.get_mut(&first.parent) {
                Some(mut entries) => entries.extend(stored),
                None => self.relationships.entry(first.parent.clone()).or_default().extend(stored),
            }
        }
        Ok(())
    }

    fn assemble_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships> {
        let entries = self.relationships.remove(parent_urn).map(|(_, entries)| entries).unwrap_or_default();
        Ok(joined_in_source_order(entries))
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
        dashmap_store::{DashMapRelationshipStore, ParentEntries, StoredTarget},
        relationship_edge::{FragmentEdges, RelationshipEdge},
        store::{
            RelationshipStore,
            tests::{assert_honours_the_contract, edges, in_order, record},
        },
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;
    use smallvec::SmallVec;
    use std::{mem, sync::Arc};
    use urn_rs::Urn;

    #[test]
    fn honours_the_cross_backend_contract() {
        assert_honours_the_contract(|| Box::new(DashMapRelationshipStore::new()));
    }

    #[test]
    fn a_target_grows_by_half_its_position_and_a_parent_s_bucket_does_not_grow() {
        assert_eq!(
            mem::size_of::<StoredTarget>(),
            mem::size_of::<(Arc<str>, Urn)>() + mem::size_of::<SourcePosition>() / 2
        );
        assert!(mem::size_of::<ParentEntries>() < mem::size_of::<SmallVec<[(Arc<str>, Urn); 4]>>());
    }

    #[test]
    fn assembling_a_parent_drains_its_relationships_and_leaves_other_parents_stored() {
        let store = DashMapRelationshipStore::new();
        let parent: Urn = "urn:ngsi-ld:Parent:1".parse().unwrap();
        let other: Urn = "urn:ngsi-ld:Parent:2".parse().unwrap();
        let mut written: Vec<RelationshipEdge> = edges(parent.as_str(), "hasPart", &["urn:ngsi-ld:Child:A"]);
        written.extend(edges(other.as_str(), "hasPart", &["urn:ngsi-ld:Child:B"]));

        store
            .add_edges(FragmentEdges {
                position: record(0),
                edges: &written,
            })
            .unwrap();

        assert_eq!(in_order(&store.assemble_relationships(&parent).unwrap(), "hasPart"), ["urn:ngsi-ld:Child:A"]);
        assert!(store.relationships.get(&parent).is_none());
        assert!(store.assemble_relationships(&parent).unwrap().is_empty());
        assert_eq!(in_order(&store.assemble_relationships(&other).unwrap(), "hasPart"), ["urn:ngsi-ld:Child:B"]);
    }
}

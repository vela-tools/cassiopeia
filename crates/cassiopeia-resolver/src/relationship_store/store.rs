use crate::{
    relationship_store::{error::Result, relationship_edge::FragmentEdges},
    store_write_strategy::StoreWriteStrategy,
};
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
/// [`IndexMap`] so the keys come back in the order the source first names them, the same from either
/// store.
///
/// Each key's targets are a set in the order of the source: when several records resolve to one
/// entity, each repeats the edges it shares with the others, and the entity links each target once,
/// at the place of the earliest record that names it. The targets are URNs read from source data, so
/// the set keeps the standard library's seeded hasher.
pub type StoredRelationships = IndexMap<String, IndexSet<Urn>, RandomState>;

/// Adds `target`, stored under the relationship key `key`, to `relationships` after every target
/// before it in the order of the source: a target already there keeps its earlier place.
pub(crate) fn join(relationships: &mut StoredRelationships, key: &str, target: Urn) {
    // A key already met takes the target without allocating its text again.
    match relationships.get_mut(key) {
        Some(known) => {
            known.insert(target);
        }
        None => {
            relationships.insert(key.to_owned(), IndexSet::from([target]));
        }
    }
}

/// Stores parent-child relationships discovered during resolution.
///
/// Relationships are written via [`add_edges`](Self::add_edges) and read back once per parent, at
/// entity assembly, via [`assemble_relationships`](Self::assemble_relationships).
///
/// The stages before the store run in parallel, so the fragments of one entity reach it in whatever
/// order their threads finish them. A store therefore keeps the position of the fragment beside
/// every edge and reads one parent's targets back in the order of those positions, not in the order
/// they were written: every run, on either store, links the same targets in the same order.
pub trait RelationshipStore: Debug + Send + Sync + 'static {
    /// The write pattern this backend is optimized for.
    fn write_strategy(&self) -> StoreWriteStrategy;

    /// Records the parent-child edges one fragment gives, each beside the fragment's position.
    ///
    /// A fragment records all its edges in one call, and no two fragments share a position, so no
    /// two calls give the same position.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store write fails.
    fn add_edges(&self, edges: FragmentEdges<'_>) -> Result<()>;

    /// Records the edges of many fragments in a single call.
    ///
    /// Disk-backed implementations coalesce the writes into one transaction rather than issuing one
    /// I/O per fragment. The default implementation falls back to [`add_edges`](Self::add_edges) per
    /// fragment.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store write fails.
    fn add_edge_batch(&self, batch: &[FragmentEdges<'_>]) -> Result<()> {
        for edges in batch {
            self.add_edges(*edges)?;
        }
        Ok(())
    }

    /// Assembles the relationships stored for the parent `parent_urn`.
    ///
    /// Returns a [`StoredRelationships`] map from relationship key to the distinct child URNs in the
    /// order of the source: by the positions their edges were recorded at, the edges of one fragment
    /// in the order it gave them, each child once at its earliest place. The map is empty when the
    /// parent has none.
    ///
    /// **Assembly consumes the parent's relationships.** Entity assembly visits each parent once and
    /// calls [`destroy`](Self::destroy) right after, so a store is free to release them as they are
    /// read, and each backend does what is cheapest for it. An in-memory store drains them: moving
    /// the targets out spares a copy of each, and their memory is freed as assembly progresses, so
    /// asking twice for one parent yields its relationships once and an empty map after. A
    /// disk-backed store leaves them in place: removing them would only write to a file deleted
    /// next, and would queue the parallel assembly threads behind redb's single writer, whereas its
    /// read transactions run side by side.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipStoreError`](crate::relationship_store::error::RelationshipStoreError)
    /// when the store cannot be read.
    fn assemble_relationships(&self, parent_urn: &Urn) -> Result<StoredRelationships>;

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
    use crate::relationship_store::{
        relationship_edge::{FragmentEdges, RelationshipEdge},
        store::{RelationshipStore, StoredRelationships, join},
    };
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    /// The parent every contract edge is recorded on.
    const PARENT: &str = "urn:ngsi-ld:Stop:S1";

    /// The children stored under `property`, in the order the set holds them: an `IndexSet`
    /// compares equal to any set of the same members, so order is asserted on the sequence.
    pub(crate) fn in_order<'a>(relationships: &'a StoredRelationships, property: &str) -> Vec<&'a str> {
        relationships.get(property).into_iter().flatten().map(|target| target.as_str()).collect()
    }

    /// The position of the record `record` records into the first input.
    pub(crate) fn record(record: usize) -> SourcePosition {
        SourcePosition::first_of(0).advanced(record)
    }

    /// The edges from `parent` to each of `children`, under `property`, in the order given.
    pub(crate) fn edges(parent: &str, property: &str, children: &[&str]) -> Vec<RelationshipEdge> {
        children
            .iter()
            .map(|child| RelationshipEdge {
                parent: parent.parse().unwrap(),
                property: property.to_owned(),
                child: child.parse().unwrap(),
            })
            .collect()
    }

    /// Records the edges the fragment at `position` gives [`PARENT`]: under each key, its children
    /// in the order given.
    fn add(store: &dyn RelationshipStore, position: SourcePosition, given: &[(&str, &[&str])]) {
        let edges: Vec<RelationshipEdge> = given.iter().flat_map(|(property, children)| edges(PARENT, property, children)).collect();
        store.add_edges(FragmentEdges { position, edges: &edges }).unwrap();
    }

    /// Asserts that the relationships assembled for [`PARENT`] hold `expected`, each key's children in
    /// order, the keys in order too.
    fn assert_reads_back(store: &dyn RelationshipStore, expected: &[(&str, &[&str])]) {
        let parent: Urn = PARENT.parse().unwrap();
        let relationships = store.assemble_relationships(&parent).unwrap();
        let found: Vec<(&str, Vec<&str>)> = relationships.keys().map(|key| (key.as_str(), in_order(&relationships, key))).collect();
        let expected: Vec<(&str, Vec<&str>)> = expected.iter().map(|(key, children)| (*key, children.to_vec())).collect();
        assert_eq!(found, expected);
    }

    /// Cross-backend contract: targets written out of the order of the source come back in it.
    fn assert_reads_targets_back_in_source_order(store: &dyn RelationshipStore) {
        // The third record, then the first, then the second: a store that kept the order of writes
        // would read `U3 U4 U1 U2`.
        add(store, record(2), &[("servedBy", &["urn:ngsi-ld:Line:U3", "urn:ngsi-ld:Line:U4"])]);
        add(store, record(0), &[("servedBy", &["urn:ngsi-ld:Line:U1"])]);
        add(store, record(1), &[("servedBy", &["urn:ngsi-ld:Line:U2"])]);

        assert_reads_back(
            store,
            &[(
                "servedBy",
                &["urn:ngsi-ld:Line:U1", "urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U3", "urn:ngsi-ld:Line:U4"],
            )],
        );
    }

    /// Cross-backend contract: one fragment's targets keep the order it gave them, never sorted by
    /// child.
    fn assert_keeps_a_fragment_s_targets_in_the_order_it_gave_them(store: &dyn RelationshipStore) {
        add(
            store,
            record(1),
            &[("servedBy", &["urn:ngsi-ld:Line:Z", "urn:ngsi-ld:Line:A", "urn:ngsi-ld:Line:M"])],
        );
        add(store, record(0), &[("servedBy", &["urn:ngsi-ld:Line:Y", "urn:ngsi-ld:Line:B"])]);

        assert_reads_back(
            store,
            &[(
                "servedBy",
                &[
                    "urn:ngsi-ld:Line:Y",
                    "urn:ngsi-ld:Line:B",
                    "urn:ngsi-ld:Line:Z",
                    "urn:ngsi-ld:Line:A",
                    "urn:ngsi-ld:Line:M",
                ],
            )],
        );
    }

    /// Cross-backend contract: a target several records name stands once, at the place of the
    /// earliest of them, whichever was written first.
    fn assert_keeps_a_repeated_target_at_its_earliest_place(store: &dyn RelationshipStore) {
        // Records naming `U2`, then `U1 U2`, written the other way round.
        add(store, record(1), &[("servedBy", &["urn:ngsi-ld:Line:U1", "urn:ngsi-ld:Line:U2"])]);
        add(store, record(0), &[("servedBy", &["urn:ngsi-ld:Line:U2"])]);
        // A `Relationship` keeps its first target: the earliest record's, written last here.
        add(store, record(4), &[("building", &["urn:ngsi-ld:Building:B2"])]);
        add(store, record(5), &[("building", &["urn:ngsi-ld:Building:B3"])]);
        add(store, record(3), &[("building", &["urn:ngsi-ld:Building:B1"])]);

        assert_reads_back(
            store,
            &[
                ("servedBy", &["urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U1"]),
                ("building", &["urn:ngsi-ld:Building:B1", "urn:ngsi-ld:Building:B2", "urn:ngsi-ld:Building:B3"]),
            ],
        );
    }

    /// Cross-backend contract: the keys come back in the order the source first names them, each
    /// with its own targets, a nested path and each instance of a multi-attribute relationship apart.
    fn assert_keeps_each_key_apart_in_source_order(store: &dyn RelationshipStore) {
        add(
            store,
            record(1),
            &[
                ("zone", &["urn:ngsi-ld:Zone:Z2"]),
                ("servesAirports#1", &["urn:ngsi-ld:Airport:A4", "urn:ngsi-ld:Airport:A3"]),
                ("hasLeadActor.playsCharacter", &["urn:ngsi-ld:Character:C2"]),
            ],
        );
        add(
            store,
            record(0),
            &[
                ("servesAirports#1", &["urn:ngsi-ld:Airport:A3"]),
                ("hasLeadActor.playsCharacter", &["urn:ngsi-ld:Character:C1"]),
                ("zone", &["urn:ngsi-ld:Zone:Z1"]),
            ],
        );

        assert_reads_back(
            store,
            &[
                ("servesAirports#1", &["urn:ngsi-ld:Airport:A3", "urn:ngsi-ld:Airport:A4"]),
                ("hasLeadActor.playsCharacter", &["urn:ngsi-ld:Character:C1", "urn:ngsi-ld:Character:C2"]),
                ("zone", &["urn:ngsi-ld:Zone:Z1", "urn:ngsi-ld:Zone:Z2"]),
            ],
        );
    }

    /// Cross-backend contract: a batch of fragments, given out of the order of the source and with
    /// one fragment linking two parents, is read back per parent in the order of the source.
    fn assert_reads_a_batch_back_in_source_order(store: &dyn RelationshipStore) {
        let other = "urn:ngsi-ld:Stop:S2";
        let late = edges(PARENT, "servedBy", &["urn:ngsi-ld:Line:U3"]);
        let mut mixed = edges(PARENT, "servedBy", &["urn:ngsi-ld:Line:U1"]);
        mixed.extend(edges(other, "servedBy", &["urn:ngsi-ld:Line:U9", "urn:ngsi-ld:Line:U8"]));
        mixed.extend(edges(PARENT, "servedBy", &["urn:ngsi-ld:Line:U2"]));
        let early = edges(other, "servedBy", &["urn:ngsi-ld:Line:U7"]);

        store
            .add_edge_batch(&[
                FragmentEdges {
                    position: record(2),
                    edges: &late,
                },
                FragmentEdges {
                    position: record(1),
                    edges: &mixed,
                },
                FragmentEdges {
                    position: record(0),
                    edges: &early,
                },
                FragmentEdges {
                    position: record(3),
                    edges: &[],
                },
            ])
            .unwrap();

        let other: Urn = other.parse().unwrap();
        assert_eq!(
            in_order(&store.assemble_relationships(&other).unwrap(), "servedBy"),
            ["urn:ngsi-ld:Line:U7", "urn:ngsi-ld:Line:U9", "urn:ngsi-ld:Line:U8"]
        );
        assert_reads_back(store, &[("servedBy", &["urn:ngsi-ld:Line:U1", "urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U3"])]);
    }

    /// Holds `store`, made fresh for each contract by `store`, to every contract a
    /// [`RelationshipStore`] honours, so both backends are held to identical behaviour.
    pub(crate) fn assert_honours_the_contract(store: impl Fn() -> Box<dyn RelationshipStore>) {
        assert_reads_targets_back_in_source_order(store().as_ref());
        assert_keeps_a_fragment_s_targets_in_the_order_it_gave_them(store().as_ref());
        assert_keeps_a_repeated_target_at_its_earliest_place(store().as_ref());
        assert_keeps_each_key_apart_in_source_order(store().as_ref());
        assert_reads_a_batch_back_in_source_order(store().as_ref());
    }

    #[test]
    fn joining_keeps_each_key_s_first_place_and_each_target_s_first_place() {
        let mut joined = StoredRelationships::default();
        for (key, target) in [
            ("servedBy", "urn:ngsi-ld:Line:U2"),
            ("zone", "urn:ngsi-ld:Zone:Z1"),
            ("servedBy", "urn:ngsi-ld:Line:U1"),
            ("servedBy", "urn:ngsi-ld:Line:U2"),
        ] {
            join(&mut joined, key, target.parse().unwrap());
        }

        assert_eq!(joined.keys().collect::<Vec<&String>>(), ["servedBy", "zone"]);
        assert_eq!(in_order(&joined, "servedBy"), ["urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U1"]);
        assert_eq!(in_order(&joined, "zone"), ["urn:ngsi-ld:Zone:Z1"]);
    }
}

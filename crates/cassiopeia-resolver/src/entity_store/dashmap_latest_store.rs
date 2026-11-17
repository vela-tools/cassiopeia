use crate::{
    entity_store::{
        concurrent_scopes::ConcurrentScopes,
        error::{EntityStoreError, Result},
        latest_slot::{IncomingRecord, LatestSlot},
        mapping_expiry::join,
        mapping_reads::MappingReads,
        store::{AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
    },
    mapping_id::MappingId,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::RandomState;
use cassiopeia_mapping::source_reads::SourceReads;
use dashmap::DashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use urn_rs::Urn;

/// One base id's slots, one per mapping that contributed to it.
///
/// An id is fed by one mapping in the common case and by a handful at most, so a linear scan finds a
/// mapping's slot faster than a hash would, and the inline capacity of one keeps the slots inside the
/// map's own entry rather than in a table of their own on the heap.
type MappingSlots = SmallVec<[(MappingId, LatestSlot); 1]>;

/// In-memory current-state entity store using [`DashMap`] for lock-free concurrent access.
///
/// Each base [`Urn`] maps to one [`LatestSlot`] per mapping: a temporal mapping keeps the
/// greatest-`observedAt` record, a static mapping merges its records in source order. This is
/// O(mappings) per id, so a temporal id's many observations never grow the stored footprint. Each
/// slot carries the expiry its records contribute, and the entity's own is joined from them at
/// assembly. Keys compare by `Urn` equivalence, RFC 8141 section 3, the scheme-based rung of the
/// RFC 3986 section 6 comparison NGSI-LD clause 5.5.1 adopts, so ids differing only in case of the
/// scheme or NID, in percent-encoding hex case, or in r-, q- or f-components name one entity.
#[derive(Clone, Debug, Default)]
pub struct DashMapLatestEntityStore {
    scopes: ConcurrentScopes,
    data: Arc<DashMap<Urn, MappingSlots, RandomState>>,
}

impl DashMapLatestEntityStore {
    /// Creates a new, empty in-memory current-state entity store.
    #[must_use]
    pub fn new() -> DashMapLatestEntityStore {
        DashMapLatestEntityStore::default()
    }
}

impl EntityStore for DashMapLatestEntityStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::Concurrent
    }

    fn store_fragment(&self, write: FragmentWrite<'_>) -> Result<()> {
        let FragmentWrite {
            base_id,
            source_data,
            position,
            batch,
            mapping_id,
            temporal,
            timestamp,
            scope,
            expires_at,
            reads,
        } = write;

        // A repeated id is the common case: every later record of a mapping lands on a key that is
        // already present, so `get_mut` serves it without cloning a URN the map would discard, and
        // only the first record of an id pays for an owned key.
        let incoming = IncomingRecord {
            data: source_data,
            position,
            batch,
            timestamp,
            expires_at,
        };
        let unplaced = match self.data.get_mut(&base_id) {
            Some(mut per_mapping) => place(&mut per_mapping, mapping_id, incoming, reads),
            None => Some(incoming),
        };
        if let Some(incoming) = unplaced {
            let mut per_mapping = self.data.entry(base_id.clone()).or_default();
            // A concurrent writer may have created the id between the miss and this insert, so the
            // slot is looked up again rather than assumed vacant.
            if let Some(incoming) = place(&mut per_mapping, mapping_id, incoming, reads) {
                per_mapping.push((mapping_id, LatestSlot::new(incoming, temporal)));
            }
        }

        if let Some(scope) = scope {
            self.scopes.merge(base_id, scope);
        }
        Ok(())
    }

    fn get_entity_ids(&self) -> Result<Vec<Urn>> {
        Ok(self.data.iter().map(|entry| entry.key().clone()).collect())
    }

    fn assemble_entity(&self, base_id: &Urn, mappings: &dyn MappingReads) -> Result<AssembledFragments> {
        // Draining rather than reading: the scan visits each id once and the store is destroyed
        // straight after, so the retained records are moved into the emit-unit instead of copied.
        let (expires_at, units) = match self.data.remove(base_id) {
            Some((_id, per_mapping)) if !per_mapping.is_empty() => {
                let mut unit = StoredUnit::new();
                let mut expiries = Vec::with_capacity(per_mapping.len());
                for (mapping_id, slot) in per_mapping {
                    let reads = mappings.reads(mapping_id).ok_or_else(|| EntityStoreError::UnregisteredMapping {
                        entity: base_id.clone(),
                        mapping_id,
                    })?;
                    let (fragment, expiry) = slot.into_stored(reads).into_fragment(mapping_id);
                    unit.push(fragment);
                    expiries.push(expiry);
                }
                // A stable join order keeps the assembled entity deterministic across runs.
                unit.sort_by_key(|fragment| fragment.mapping_id);
                (join(expiries), vec![unit])
            }
            _ => (None, Vec::new()),
        };
        let scope = self.scopes.get(base_id);
        Ok(AssembledFragments { scope, expires_at, units })
    }

    fn count(&self) -> usize {
        self.data.len()
    }

    fn unit_count(&self) -> usize {
        // Current-state assembly emits one unit per id.
        self.data.len()
    }

    fn clear(&self) -> Result<()> {
        self.scopes.clear();
        self.data.clear();
        Ok(())
    }

    fn destroy(&self) -> Result<()> {
        self.clear()
    }

    fn clone_box(&self) -> Box<dyn EntityStore> {
        Box::new(self.clone())
    }
}

/// Folds one record into an id's slot for its mapping, counting it among the records the mapping
/// resolved to the id.
///
/// Returns `None` once the record has been absorbed. Returns the record back when the mapping has no
/// slot yet, so the caller can insert it under a key it owns; handing it back rather than inserting
/// here keeps the owned-key clone on the one path that needs it.
fn place(per_mapping: &mut MappingSlots, mapping_id: MappingId, incoming: IncomingRecord, reads: &SourceReads) -> Option<IncomingRecord> {
    let Some((_, slot)) = per_mapping.iter_mut().find(|(known, _)| *known == mapping_id) else {
        return Some(incoming);
    };
    slot.absorb(incoming, reads);
    None
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            dashmap_latest_store::DashMapLatestEntityStore,
            record_timestamp::RecordTimestamp,
            store::{
                EntityStore,
                FragmentWrite,
                tests::{READS_EVERYTHING, assert_expiry_follows_the_retained_record, assert_joins_expiries_across_mappings, assert_merges_scopes_per_id},
            },
        },
        mapping_id::MappingId,
        test_order::{ReadsEverything, next_batch, next_position},
    };
    use cassiopeia_mapping::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    #[test]
    fn scopes_from_every_fragment_of_an_id_are_merged() {
        assert_merges_scopes_per_id(&DashMapLatestEntityStore::new());
    }

    #[test]
    fn the_expiries_of_an_id_s_mappings_are_joined() {
        assert_joins_expiries_across_mappings(&DashMapLatestEntityStore::new());
    }

    #[test]
    fn a_mapping_s_expiry_follows_the_record_it_retains() {
        assert_expiry_follows_the_retained_record(&DashMapLatestEntityStore::new());
    }

    fn write(base_id: &Urn, data: serde_json::Value, mapping: u32, temporal: bool, observed_at: Option<&str>) -> FragmentWrite<'static> {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            position: next_position(),
            batch: next_batch(),
            mapping_id: MappingId::new(mapping),
            temporal,
            timestamp: RecordTimestamp::from(observed_at.map(ObservedAt::new)),
            scope: None,
            expires_at: None,
            reads: &READS_EVERYTHING,
        }
    }

    #[test]
    fn two_mappings_for_one_id_join_into_one_unit() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        let geometry = json!({"location": {"type": "Point"}});
        let state = json!({"on": true});

        store.store_fragment(write(&id, geometry, 1, false, None)).unwrap();
        store.store_fragment(write(&id, state, 2, false, None)).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units.len(), 1);
        assert_eq!(assembled.units[0].len(), 2);
    }

    #[test]
    fn three_mappings_for_one_id_all_survive_the_join_ordered_by_mapping_id() {
        // Three fragments spill the emit-unit's inline capacity of one onto the heap; the join must
        // still return every mapping's record, in ascending mapping-id order.
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"third": 3}), 3, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"first": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"second": 2}), 2, false, None)).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units.len(), 1);
        let unit = &assembled.units[0];
        assert_eq!(unit.len(), 3);
        assert_eq!(
            unit.iter().map(|fragment| fragment.mapping_id).collect::<Vec<MappingId>>(),
            vec![MappingId::new(1), MappingId::new(2), MappingId::new(3)]
        );
        assert_eq!(unit[0].data, json!({"first": 1}));
        assert_eq!(unit[2].data, json!({"third": 3}));
    }

    #[test]
    fn a_temporal_mapping_keeps_only_the_latest_record() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        // Only the superseded record carries `battery`, so a merge rather than a replacement would
        // leave it in the retained record.
        let first = json!({"on": false, "battery": 80, "ts": "2026-04-03T22:00:20Z"});
        let middle = json!({"on": true, "ts": "2026-04-03T22:10:20Z"});
        let last = json!({"on": false, "ts": "2026-04-03T22:05:20Z"});

        store.store_fragment(write(&id, first, 1, true, Some("2026-04-03T22:00:20Z"))).unwrap();
        store.store_fragment(write(&id, middle.clone(), 1, true, Some("2026-04-03T22:10:20Z"))).unwrap();
        store.store_fragment(write(&id, last, 1, true, Some("2026-04-03T22:05:20Z"))).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units.len(), 1);
        assert_eq!(assembled.units[0].len(), 1);
        assert_eq!(assembled.units[0][0].data, middle);
    }

    #[test]
    fn a_static_mapping_deep_merges_repeated_records() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"b": 2}), 1, false, None)).unwrap();

        let assembled = store.assemble_entity(&id, &ReadsEverything).unwrap();
        assert_eq!(assembled.units[0][0].data, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn each_mapping_slot_counts_every_record_it_absorbed() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"b": 2}), 1, false, None)).unwrap();
        store
            .store_fragment(write(&id, json!({"on": true}), 2, true, Some("2026-04-03T22:00:20Z")))
            .unwrap();
        store
            .store_fragment(write(&id, json!({"on": false}), 2, true, Some("2026-04-03T21:00:20Z")))
            .unwrap();
        store.store_fragment(write(&id, json!({"c": 3}), 3, false, None)).unwrap();

        let unit = &store.assemble_entity(&id, &ReadsEverything).unwrap().units[0];
        let counts: Vec<u64> = unit.iter().map(|fragment| fragment.records.get()).collect();
        // A stale observation is discarded yet still counted: it resolved to the id all the same.
        assert_eq!(counts, [2, 2, 1]);
    }

    #[test]
    fn a_slot_remembers_the_earliest_field_its_records_disagreed_on() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");

        store.store_fragment(write(&id, json!({"a": 1, "z": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"z": 2, "b": 2}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"a": 3}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"c": 1}), 2, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"c": 1, "d": 4}), 2, false, None)).unwrap();

        let unit = &store.assemble_entity(&id, &ReadsEverything).unwrap().units[0];
        let conflicts: Vec<Option<String>> = unit.iter().map(|fragment| fragment.conflict.as_ref().map(ToString::to_string)).collect();
        assert_eq!(conflicts, [Some("a".to_string()), None]);
        assert_eq!(unit[0].data, json!({"a": 1, "z": 1, "b": 2}));
    }

    #[test]
    fn one_unit_per_id_and_a_missing_id_assembles_to_nothing() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();

        assert_eq!(store.count(), 1);
        assert_eq!(store.unit_count(), 1);
        assert!(
            store
                .assemble_entity(&urn("urn:ngsi-ld:Camera:absent"), &ReadsEverything)
                .unwrap()
                .units
                .is_empty()
        );
    }

    #[test]
    fn assembly_drains_the_id_rather_than_copying_it_out() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"b": 2}), 2, false, None)).unwrap();

        assert_eq!(store.assemble_entity(&id, &ReadsEverything).unwrap().units[0].len(), 2);

        // The retained records moved into the emit-unit, so the store holds no second copy.
        assert_eq!(store.count(), 0);
        assert!(store.assemble_entity(&id, &ReadsEverything).unwrap().units.is_empty());
    }
}

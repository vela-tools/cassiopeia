use crate::{
    entity_store::{
        error::Result,
        merge::deep_merge,
        store::{AssembledFragment, AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
        supersession::supersedes,
    },
    mapping_id::MappingId,
    store_write_strategy::StoreWriteStrategy,
};
use ahash::{AHashMap, RandomState};
use cassiopeia_mapping::observed_at::ObservedAt;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use urn_rs::Urn;

/// One mapping's retained fragment for a base id under the current-state model.
#[derive(Debug)]
struct LatestFragment {
    /// The retained source record.
    data: Value,
    /// The record-level `observedAt` of the retained record, for ranking a temporal mapping's records.
    observed_at: Option<ObservedAt>,
}

/// In-memory current-state entity store using [`DashMap`] for lock-free concurrent access.
///
/// Each base [`Urn`] maps to one retained fragment per mapping: a temporal mapping keeps the
/// greatest-`observedAt` record, a static mapping deep-merges its records. This is O(mappings) per id,
/// so a temporal id's many observations never grow the stored footprint. The `exact-eq` feature of
/// `urn-rs` makes `Urn`'s `Hash`/`Eq` cover the full normalized URN, so keys hash without a
/// `to_string()` round-trip.
#[derive(Clone, Debug)]
pub struct DashMapLatestEntityStore {
    scopes: Arc<DashMap<Urn, NgsiLdScope, RandomState>>,
    data: Arc<DashMap<Urn, AHashMap<MappingId, LatestFragment>, RandomState>>,
}

impl DashMapLatestEntityStore {
    /// Creates a new, empty in-memory current-state entity store.
    #[must_use]
    pub fn new() -> DashMapLatestEntityStore {
        DashMapLatestEntityStore {
            scopes: Arc::new(DashMap::with_hasher(RandomState::new())),
            data: Arc::new(DashMap::with_hasher(RandomState::new())),
        }
    }
}

impl EntityStore for DashMapLatestEntityStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::Concurrent
    }

    fn store_fragment(&self, write: FragmentWrite) -> Result<()> {
        let FragmentWrite {
            base_id,
            source_data,
            mapping_id,
            temporal,
            observed_at,
            scope,
        } = write;

        // A repeated id is the common case: every later record of a mapping lands on a key that is
        // already present, so `get_mut` serves it without cloning a URN the map would discard, and
        // only the first record of an id pays for an owned key.
        let unplaced = match self.data.get_mut(&base_id) {
            Some(mut per_mapping) => merge_fragment(&mut per_mapping, mapping_id, source_data, temporal, observed_at),
            None => Some((source_data, observed_at)),
        };
        if let Some((source_data, observed_at)) = unplaced {
            let mut per_mapping = self.data.entry(base_id.clone()).or_default();
            // A concurrent writer may have created the id between the miss and this insert, so the
            // merge rule is re-applied rather than assuming the mapping slot is still vacant.
            if let Some((source_data, observed_at)) = merge_fragment(&mut per_mapping, mapping_id, source_data, temporal, observed_at) {
                per_mapping.insert(
                    mapping_id,
                    LatestFragment {
                        data: source_data,
                        observed_at,
                    },
                );
            }
        }

        if let Some(scope) = scope {
            self.scopes.insert(base_id, scope);
        }
        Ok(())
    }

    fn get_entity_ids(&self) -> Result<Vec<Urn>> {
        Ok(self.data.iter().map(|entry| entry.key().clone()).collect())
    }

    fn assemble_entity(&self, base_id: &Urn) -> Result<AssembledFragments> {
        // Draining rather than reading: the scan visits each id once and the store is destroyed
        // straight after, so the retained records are moved into the emit-unit instead of copied.
        let units = match self.data.remove(base_id) {
            Some((_id, per_mapping)) if !per_mapping.is_empty() => {
                let mut unit: StoredUnit = per_mapping
                    .into_iter()
                    .map(|(mapping_id, fragment)| AssembledFragment {
                        mapping_id,
                        data: fragment.data,
                    })
                    .collect();
                // A stable join order keeps the assembled entity deterministic across runs.
                unit.sort_by_key(|fragment| fragment.mapping_id);
                vec![unit]
            }
            _ => Vec::new(),
        };
        let scope = self.scopes.get(base_id).map(|entry| entry.clone());
        Ok(AssembledFragments { scope, units })
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

impl Default for DashMapLatestEntityStore {
    fn default() -> DashMapLatestEntityStore {
        DashMapLatestEntityStore::new()
    }
}

/// Folds one record into an id's retained fragment for its mapping under the current-state rule.
///
/// Returns `None` once the record has been absorbed: merged, superseding, or deliberately discarded
/// as stale. Returns the record back when the mapping has no slot yet, so the caller can insert it
/// under a key it owns; handing it back rather than inserting here keeps the owned-key clone on the
/// one path that needs it.
fn merge_fragment(
    per_mapping: &mut AHashMap<MappingId, LatestFragment>,
    mapping_id: MappingId,
    source_data: Value,
    temporal: bool,
    observed_at: Option<ObservedAt>,
) -> Option<(Value, Option<ObservedAt>)> {
    let Some(existing) = per_mapping.get_mut(&mapping_id) else {
        return Some((source_data, observed_at));
    };

    if temporal {
        // A temporal mapping's records are observations of one id: keep the latest.
        let new = observed_at.as_ref().map(ObservedAt::as_str);
        if supersedes(new, existing.observed_at.as_ref().map(ObservedAt::as_str)) {
            existing.data = source_data;
            existing.observed_at = observed_at;
        }
    } else {
        // A static mapping's records refine one entity: merge them.
        deep_merge(&mut existing.data, &source_data);
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            dashmap_latest_store::DashMapLatestEntityStore,
            store::{EntityStore, FragmentWrite},
        },
        mapping_id::MappingId,
    };
    use cassiopeia_mapping::observed_at::ObservedAt;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn write(base_id: &Urn, data: serde_json::Value, mapping: u32, temporal: bool, observed_at: Option<&str>) -> FragmentWrite {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            mapping_id: MappingId::new(mapping),
            temporal,
            observed_at: observed_at.map(ObservedAt::new),
            scope: None,
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

        let assembled = store.assemble_entity(&id).unwrap();
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

        let assembled = store.assemble_entity(&id).unwrap();
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
        let first = json!({"on": false, "ts": "2026-04-03T22:00:20Z"});
        let middle = json!({"on": true, "ts": "2026-04-03T22:10:20Z"});
        let last = json!({"on": false, "ts": "2026-04-03T22:05:20Z"});

        store.store_fragment(write(&id, first, 1, true, Some("2026-04-03T22:00:20Z"))).unwrap();
        store.store_fragment(write(&id, middle.clone(), 1, true, Some("2026-04-03T22:10:20Z"))).unwrap();
        store.store_fragment(write(&id, last, 1, true, Some("2026-04-03T22:05:20Z"))).unwrap();

        let assembled = store.assemble_entity(&id).unwrap();
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

        let assembled = store.assemble_entity(&id).unwrap();
        assert_eq!(assembled.units[0][0].data, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn one_unit_per_id_and_a_missing_id_assembles_to_nothing() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();

        assert_eq!(store.count(), 1);
        assert_eq!(store.unit_count(), 1);
        assert!(store.assemble_entity(&urn("urn:ngsi-ld:Camera:absent")).unwrap().units.is_empty());
    }

    #[test]
    fn assembly_drains_the_id_rather_than_copying_it_out() {
        let store = DashMapLatestEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"a": 1}), 1, false, None)).unwrap();
        store.store_fragment(write(&id, json!({"b": 2}), 2, false, None)).unwrap();

        assert_eq!(store.assemble_entity(&id).unwrap().units[0].len(), 2);

        // The retained records moved into the emit-unit, so the store holds no second copy.
        assert_eq!(store.count(), 0);
        assert!(store.assemble_entity(&id).unwrap().units.is_empty());
    }
}

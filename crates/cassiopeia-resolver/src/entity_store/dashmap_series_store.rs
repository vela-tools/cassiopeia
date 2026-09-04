use crate::{
    entity_store::{
        error::Result,
        store::{AssembledFragment, AssembledFragments, EntityStore, FragmentWrite, StoredUnit},
    },
    store_write_strategy::StoreWriteStrategy,
};
use ahash::RandomState;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use dashmap::DashMap;
use smallvec::smallvec;
use std::sync::Arc;
use urn_rs::Urn;

/// In-memory series entity store using [`DashMap`] for lock-free concurrent access.
///
/// Each base [`Urn`] maps to an append-only list of every fragment stored for it, in arrival order.
/// Assembly returns one emit-unit per fragment, so a temporal id's observations each become a
/// single-instance entity the fold stage later folds into one `EntityTemporal` (ETSI GS CIM 009
/// v1.9.1 clause 5.2.20).
#[derive(Clone, Debug)]
pub struct DashMapSeriesEntityStore {
    scopes: Arc<DashMap<Urn, NgsiLdScope, RandomState>>,
    data: Arc<DashMap<Urn, Vec<AssembledFragment>, RandomState>>,
}

impl DashMapSeriesEntityStore {
    /// Creates a new, empty in-memory series entity store.
    #[must_use]
    pub fn new() -> DashMapSeriesEntityStore {
        DashMapSeriesEntityStore {
            scopes: Arc::new(DashMap::with_hasher(RandomState::new())),
            data: Arc::new(DashMap::with_hasher(RandomState::new())),
        }
    }
}

impl EntityStore for DashMapSeriesEntityStore {
    fn write_strategy(&self) -> StoreWriteStrategy {
        StoreWriteStrategy::Concurrent
    }

    fn store_fragment(&self, write: FragmentWrite) -> Result<()> {
        let FragmentWrite {
            base_id,
            source_data,
            mapping_id,
            scope,
            ..
        } = write;
        let fragment = AssembledFragment { mapping_id, data: source_data };

        // Every observation after an id's first finds the key already present, which is the whole
        // shape of a series run. `get_mut` serves that case without cloning a URN the map would only
        // discard, leaving the clone to the one insert per id that actually needs an owned key.
        let unplaced = match self.data.get_mut(&base_id) {
            Some(mut existing) => {
                existing.push(fragment);
                None
            }
            None => Some(fragment),
        };
        if let Some(fragment) = unplaced {
            self.data.entry(base_id.clone()).or_default().push(fragment);
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
        // straight after, so every stored record is moved into its emit-unit. Copying them here
        // instead would deep-clone the entire dataset a second time, on top of the copy the store
        // already holds.
        let units: Vec<StoredUnit> = self
            .data
            .remove(base_id)
            .map(|(_id, fragments)| fragments.into_iter().map(|fragment| smallvec![fragment]).collect())
            .unwrap_or_default();
        let scope = self.scopes.get(base_id).map(|entry| entry.clone());
        Ok(AssembledFragments { scope, units })
    }

    fn count(&self) -> usize {
        self.data.len()
    }

    fn unit_count(&self) -> usize {
        self.data.iter().map(|entry| entry.value().len()).sum()
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

impl Default for DashMapSeriesEntityStore {
    fn default() -> DashMapSeriesEntityStore {
        DashMapSeriesEntityStore::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_store::{
            dashmap_series_store::DashMapSeriesEntityStore,
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

    fn write(base_id: &Urn, data: serde_json::Value, observed_at: &str) -> FragmentWrite {
        FragmentWrite {
            base_id: base_id.clone(),
            source_data: data,
            mapping_id: MappingId::new(1),
            temporal: true,
            observed_at: Some(ObservedAt::new(observed_at)),
            scope: None,
        }
    }

    #[test]
    fn three_observations_become_three_units_in_arrival_order() {
        let store = DashMapSeriesEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        let first = json!({"on": true, "ts": "2026-04-03T22:00:20Z"});
        let second = json!({"on": false, "ts": "2026-04-03T22:05:20Z"});
        let third = json!({"on": true, "ts": "2026-04-03T22:10:20Z"});

        store.store_fragment(write(&id, first.clone(), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, second.clone(), "2026-04-03T22:05:20Z")).unwrap();
        store.store_fragment(write(&id, third.clone(), "2026-04-03T22:10:20Z")).unwrap();
        assert_eq!(store.count(), 1);
        assert_eq!(store.unit_count(), 3);

        let assembled = store.assemble_entity(&id).unwrap();
        assert_eq!(assembled.units.len(), 3);
        assert_eq!(assembled.units[0][0].data, first);
        assert_eq!(assembled.units[1][0].data, second);
        assert_eq!(assembled.units[2][0].data, third);
    }

    #[test]
    fn assembly_drains_the_id_rather_than_copying_it_out() {
        let store = DashMapSeriesEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"on": true}), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, json!({"on": false}), "2026-04-03T22:05:20Z")).unwrap();

        assert_eq!(store.assemble_entity(&id).unwrap().units.len(), 2);

        // The records moved into the emit-units, so nothing is left holding a second copy.
        assert_eq!(store.count(), 0);
        assert_eq!(store.unit_count(), 0);
        assert!(store.assemble_entity(&id).unwrap().units.is_empty());
    }

    #[test]
    fn a_second_observation_appends_without_re_inserting_the_id() {
        // The append path takes `get_mut` rather than `entry`, so this covers that both paths land
        // the fragment under the same key and in arrival order.
        let store = DashMapSeriesEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"seq": 1}), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, json!({"seq": 2}), "2026-04-03T22:05:20Z")).unwrap();
        store.store_fragment(write(&id, json!({"seq": 3}), "2026-04-03T22:10:20Z")).unwrap();

        assert_eq!(store.count(), 1);
        let assembled = store.assemble_entity(&id).unwrap();
        let sequence: Vec<i64> = assembled
            .units
            .iter()
            .filter_map(|unit| unit.first().and_then(|fragment| fragment.data.get("seq")?.as_i64()))
            .collect();
        assert_eq!(sequence, vec![1, 2, 3]);
    }

    #[test]
    fn get_entity_ids_returns_one_entry_per_base_id() {
        let store = DashMapSeriesEntityStore::new();
        let id = urn("urn:ngsi-ld:Camera:1");
        store.store_fragment(write(&id, json!({"on": true}), "2026-04-03T22:00:20Z")).unwrap();
        store.store_fragment(write(&id, json!({"on": false}), "2026-04-03T22:05:20Z")).unwrap();

        assert_eq!(store.get_entity_ids().unwrap(), vec![id]);
    }
}

use crate::mapping_id::MappingId;
use ahash::RandomState;
use cassiopeia_mapping::{mapping::Mapping, template::run_variables::RunVariables};
use dashmap::DashMap;
use smallvec::SmallVec;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// A mapping as one input renders its records through it: the mapping, and the run variables the
/// input reads them with.
///
/// Two inputs may share a mapping file and still set different variables, so the same mapping
/// renders their records differently. A fragment's record is only ever rendered through the bound
/// mapping of the input it came from.
#[derive(Debug, Clone)]
pub struct BoundMapping {
    /// The mapping that renders the records.
    mapping: Arc<Mapping>,
    /// The run variables the records are read with.
    variables: RunVariables,
}

impl BoundMapping {
    /// The mapping that renders the records.
    #[must_use]
    pub const fn mapping(&self) -> &Arc<Mapping> {
        &self.mapping
    }

    /// The run variables the records are read with.
    #[must_use]
    pub const fn variables(&self) -> &RunVariables {
        &self.variables
    }
}

/// The variables one mapping has been interned with so far, each with its id. One entry in the
/// common case of a mapping every input reads with the same variables.
type MappingReadings = SmallVec<[(RunVariables, MappingId); 1]>;

/// Interns each mapping, together with the run variables an input reads its records with, so each
/// distinct pair carries a compact [`MappingId`] through the entity store instead of a per-URN
/// pointer to the full configuration.
///
/// A mapping is told apart by `Arc::as_ptr`: the pipeline compiles one `Arc<Mapping>` per mapping
/// file and clones it onto every fragment it produces, so pointer identity is stable across calls for
/// the lifetime of a run. Variables are told apart by value, so two inputs that share a mapping and
/// set equal variables, each holding its own copy, render their records alike and share an id, and
/// their records for the same entity still merge into one slot.
///
/// Repeated lookups go through a sharded map keyed by both addresses, so concurrent resolver workers
/// do not contend on a single lock. Ids are handed out from `1` upward; `MappingId(0)` stays reserved
/// as the entity store's "no fragment" sentinel.
#[derive(Debug, Default)]
pub struct MappingRegistry {
    /// Each pair of mapping and variables addresses seen, with its id and a handle on the variables
    /// that keeps their address from being reused while it is a key.
    by_address: DashMap<(usize, usize), (MappingId, RunVariables), RandomState>,
    /// Every set of variables each mapping has been interned with, by the mapping's address.
    by_mapping: DashMap<usize, MappingReadings, RandomState>,
    /// The mapping and variables each id stands for.
    by_id: DashMap<u32, BoundMapping, RandomState>,
    /// The last id handed out.
    next: AtomicU32,
}

impl MappingRegistry {
    /// Creates a new, empty registry.
    #[must_use]
    pub fn new() -> MappingRegistry {
        MappingRegistry::default()
    }

    /// Returns the id for `mapping` read with `variables`, assigning a fresh one on first sight of a
    /// pair whose variables differ from every set the mapping was interned with before.
    ///
    /// Interning the same pair again, or the same mapping with equal variables, returns the id
    /// already assigned to it.
    pub fn intern(&self, mapping: &Arc<Mapping>, variables: &RunVariables) -> MappingId {
        let mapping_address = Arc::as_ptr(mapping) as usize;
        let key = (mapping_address, variables.address());
        if let Some(known) = self.by_address.get(&key) {
            return known.0;
        }

        // The mapping's shard is locked while its variables are compared and an id is handed out, so
        // two workers meeting equal variables first at the same time agree on one id.
        let mut readings = self.by_mapping.entry(mapping_address).or_default();
        let known = readings.iter().find(|(known, _)| known == variables).map(|(_, id)| *id);
        let id = known.unwrap_or_else(|| {
            let id = self.allocate(mapping, variables);
            // The mapping's list keeps its own handle on the variables it compares against.
            readings.push((variables.clone(), id));
            id
        });
        // The handle keeps the variables' address from being reused for different variables while it
        // is a key here.
        self.by_address.insert(key, (id, variables.clone()));
        id
    }

    /// Hands out the next id for `mapping` read with `variables`, and records what it stands for.
    fn allocate(&self, mapping: &Arc<Mapping>, variables: &RunVariables) -> MappingId {
        // `fetch_add` returns the previous value, so the first id is 1 and the `0` sentinel is never
        // handed out.
        let raw = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        // Cloning the handles only bumps reference counts; the registry keeps the mapping and the
        // variables alive so assembly can resolve the id back to them.
        let bound = BoundMapping {
            mapping: Arc::clone(mapping),
            variables: variables.clone(),
        };
        self.by_id.insert(raw, bound);
        MappingId::new(raw)
    }

    /// Resolves an id back to its mapping and variables, or `None` for the `0` sentinel or an id
    /// that was never interned.
    pub fn resolve(&self, id: MappingId) -> Option<BoundMapping> {
        let raw = id.as_u32();
        if raw == 0 {
            return None;
        }
        // The registry keeps its own handles, so the caller gets clones of them.
        self.by_id.get(&raw).map(|entry| entry.value().clone())
    }

    /// Returns the number of distinct mapping and variables pairs interned so far.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether nothing has been interned yet.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::{mapping_id::MappingId, mapping_registry::MappingRegistry};
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{run_variables::RunVariables, runner::TemplateRunner},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Map, json};
    use std::{path::Path, sync::Arc};

    const STATION: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ vars.network }}-{{ id }}" },
        attributes: { name: { source: "{{ name }}" } },
    }"#;

    fn mapping() -> Arc<Mapping> {
        Arc::new(Mapping::from_json5(STATION, Path::new("station.json5"), &mut TemplateRunner::new()).unwrap())
    }

    /// Run variables setting `network` to `value`.
    fn network(value: &str) -> RunVariables {
        let mut variables = Map::new();
        variables.insert("network".to_string(), json!(value));
        RunVariables::new(variables)
    }

    #[test]
    fn the_same_mapping_read_with_the_same_variables_keeps_one_id() {
        let registry = MappingRegistry::new();
        let station = mapping();
        let variables = network("bikes");

        let first = registry.intern(&station, &variables);

        assert_eq!(registry.intern(&station, &variables), first);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn equal_variables_held_apart_share_an_id() {
        let registry = MappingRegistry::new();
        let station = mapping();

        let first = registry.intern(&station, &network("bikes"));

        assert_eq!(registry.intern(&station, &network("bikes")), first);
        assert_eq!(
            registry.intern(&station, &RunVariables::none()),
            registry.intern(&station, &RunVariables::none())
        );
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn one_mapping_read_with_different_variables_gets_one_id_per_set() {
        let registry = MappingRegistry::new();
        let station = mapping();

        let bikes = registry.intern(&station, &network("bikes"));
        let scooters = registry.intern(&station, &network("scooters"));

        assert_ne!(bikes, scooters);
        assert_eq!(registry.resolve(bikes).unwrap().variables(), &network("bikes"));
        assert_eq!(registry.resolve(scooters).unwrap().variables(), &network("scooters"));
        assert!(Arc::ptr_eq(registry.resolve(scooters).unwrap().mapping(), &station));
    }

    #[test]
    fn distinct_mappings_get_distinct_ids_whatever_their_variables() {
        let registry = MappingRegistry::new();

        assert_ne!(
            registry.intern(&mapping(), &RunVariables::none()),
            registry.intern(&mapping(), &RunVariables::none())
        );
    }

    #[test]
    fn the_sentinel_and_an_unknown_id_resolve_to_nothing() {
        let registry = MappingRegistry::new();
        registry.intern(&mapping(), &RunVariables::none());

        assert!(registry.resolve(MappingId::new(0)).is_none());
        assert!(registry.resolve(MappingId::new(7)).is_none());
        assert!(!registry.is_empty());
    }
}

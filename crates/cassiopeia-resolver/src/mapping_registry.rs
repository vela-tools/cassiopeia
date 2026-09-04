use crate::mapping_id::MappingId;
use ahash::RandomState;
use cassiopeia_mapping::mapping::Mapping;
use dashmap::{DashMap, mapref::entry::Entry};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// Interns `Arc<Mapping>` values so each distinct mapping carries a compact
/// [`MappingId`] through the entity store instead of a per-URN pointer to the
/// full configuration.
///
/// Deduplication uses `Arc::as_ptr`: the pipeline wires a single `Arc<Mapping>`
/// per input and clones it onto every produced fragment, so pointer identity is
/// stable across calls for the lifetime of a run.
///
/// The registry uses two sharded maps and a monotonic counter, so concurrent resolver workers do
/// not contend on a single lock. Ids are handed out from `1` upward; `MappingId(0)` stays reserved
/// as the entity store's "no fragment" sentinel.
#[derive(Debug, Default)]
pub struct MappingRegistry {
    by_ptr: DashMap<usize, MappingId, RandomState>,
    by_id: DashMap<u32, Arc<Mapping>, RandomState>,
    next: AtomicU32,
}

impl MappingRegistry {
    /// Creates a new, empty registry.
    #[must_use]
    pub fn new() -> MappingRegistry {
        MappingRegistry::default()
    }

    /// Returns the id for `mapping`, assigning a fresh one on first sight.
    ///
    /// Interning the same `Arc` again returns the id already assigned to it.
    pub fn intern(&self, mapping: &Arc<Mapping>) -> MappingId {
        let ptr = Arc::as_ptr(mapping) as usize;
        if let Some(id) = self.by_ptr.get(&ptr) {
            return *id;
        }

        // The pointer's shard is locked for the whole vacant branch, so two
        // threads racing on the same pointer cannot both allocate an id.
        match self.by_ptr.entry(ptr) {
            Entry::Occupied(existing) => *existing.get(),
            Entry::Vacant(slot) => {
                // `fetch_add` returns the previous value, so the first id is 1
                // and the `0` sentinel is never handed out.
                let raw = self.next.fetch_add(1, Ordering::Relaxed) + 1;
                let id = MappingId::new(raw);
                // Cloning an `Arc` only bumps the refcount; the registry keeps
                // the mapping alive so assembly can resolve the id back to it.
                self.by_id.insert(raw, Arc::clone(mapping));
                slot.insert(id);
                id
            }
        }
    }

    /// Resolves an id back to its mapping, or `None` for the `0` sentinel or an
    /// id that was never interned.
    pub fn resolve(&self, id: MappingId) -> Option<Arc<Mapping>> {
        let raw = id.as_u32();
        if raw == 0 {
            return None;
        }
        self.by_id.get(&raw).map(|entry| Arc::clone(entry.value()))
    }

    /// Returns the number of distinct mappings interned so far.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether no mapping has been interned yet.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

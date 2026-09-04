use serde::{Deserialize, Serialize};

/// Compact identifier for a [`Mapping`](cassiopeia_mapping::mapping::Mapping)
/// used during resolution.
///
/// The resolver interns each distinct mapping configuration (deduped by
/// `Arc::as_ptr`) into a [`MappingRegistry`](crate::mapping_registry::MappingRegistry)
/// and stores its `MappingId` alongside each fragment instead of a per-URN
/// reference to the full mapping. This cuts the per-entity heap footprint from
/// O(N) fragments back down to O(M) distinct mappings.
///
/// `MappingId(0)` is reserved as a sentinel for "no fragment stored", returned
/// by [`EntityStore::assemble_entity`](crate::entity_store::store::EntityStore::assemble_entity)
/// when a URN has no data; real ids start at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MappingId(u32);

impl MappingId {
    /// Wraps a raw id. Callers use `0` only for the "no fragment" sentinel.
    #[must_use]
    pub const fn new(id: u32) -> MappingId {
        MappingId(id)
    }

    /// Returns the raw id.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

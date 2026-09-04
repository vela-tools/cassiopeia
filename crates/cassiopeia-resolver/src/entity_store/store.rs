use crate::{entity_store::error::Result, mapping_id::MappingId, store_write_strategy::StoreWriteStrategy};
use cassiopeia_mapping::observed_at::ObservedAt;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use serde_json::Value;
use smallvec::SmallVec;
use std::fmt::Debug;
use urn_rs::Urn;

/// The inputs for storing one fragment under its base entity id.
///
/// Temporality is an attribute concern (ETSI GS CIM 009 v1.9.1 clause 4.5.5), so the store keys by
/// the base id and every mapping and every observation of that id accumulates together. `temporal`
/// and `observed_at` let a current-state store rank a temporal mapping's records by their record-level
/// `observedAt`; a series store ignores them and keeps every observation.
///
/// The write owns everything it carries. The resolver already owns the fragment it is storing and
/// has no use for it afterwards, so handing ownership over lets an in-memory store move the record
/// into place instead of deep-copying a whole `serde_json` document per fragment.
pub struct FragmentWrite {
    /// The entity id this fragment contributes to, with no temporal qualifier.
    pub base_id: Urn,
    /// The source record the fragment was expanded from.
    pub source_data: Value,
    /// The interned id of the mapping that produced the fragment.
    pub mapping_id: MappingId,
    /// Whether the producing mapping is temporal (declares an `observedAt`).
    pub temporal: bool,
    /// The record-level `observedAt`, verbatim; monotonic under string compare for a fixed format.
    /// `None` when the mapping is not temporal or the record carries no timestamp.
    pub observed_at: Option<ObservedAt>,
    /// The entity's scope, when one was resolved.
    pub scope: Option<NgsiLdScope>,
}

/// One fragment's rendering inputs, read back at assembly.
///
/// The extractor resolves the record through the mapping the id resolves to, so only the record and
/// its mapping id survive round-tripping through the store.
#[derive(Debug, Clone)]
pub struct AssembledFragment {
    /// The interned id of the mapping that produced the fragment.
    pub mapping_id: MappingId,
    /// The source record the fragment was expanded from.
    pub data: Value,
}

/// The fragments contributing to one emit-unit.
///
/// A series store puts exactly one observation in each, and a current-state store one fragment per
/// contributing mapping, so the inline capacity of one keeps the overwhelmingly common single-fragment
/// unit off the heap entirely; a series run would otherwise allocate one `Vec` per observation.
pub type StoredUnit = SmallVec<[AssembledFragment; 1]>;

/// Everything the store returns for one base entity id.
///
/// `units` holds one fragment-list per emit-unit: a current-state store returns a single unit holding
/// every mapping's fragment (the join), while a series store returns one unit per observation. An
/// empty `units` means the store held nothing for the id.
#[derive(Debug, Default)]
pub struct AssembledFragments {
    /// The entity's scope, when one was stored.
    pub scope: Option<NgsiLdScope>,
    /// One fragment-list per emit-unit.
    pub units: Vec<StoredUnit>,
}

/// Stores and retrieves entity fragments during resolution.
///
/// Entity stores accumulate source data, scope, and the id of the mapping that produced each fragment
/// under the base entity id, then assemble complete entities. Implementations must be thread-safe
/// (`Send + Sync`): fragments are stored concurrently from many resolver threads.
///
/// Two memory models exist, chosen at construction from the run's temporal target: a current-state
/// store retains only what latest-per-attribute needs (O(mappings) per id); a series store retains the
/// full append-only series per id.
pub trait EntityStore: Debug + Send + Sync + 'static {
    /// The write pattern this backend is optimized for.
    fn write_strategy(&self) -> StoreWriteStrategy;

    /// Stores one fragment under its base entity id.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// write fails.
    fn store_fragment(&self, write: FragmentWrite) -> Result<()>;

    /// Stores a batch of fragments in a single call.
    ///
    /// Disk-backed implementations coalesce the write into a single transaction, eliminating the
    /// per-fragment read-modify-write overhead of calling [`store_fragment`](Self::store_fragment) in
    /// a loop. The default implementation dispatches per entry.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// write fails.
    fn store_fragment_batch(&self, fragments: Vec<FragmentWrite>) -> Result<()> {
        for write in fragments {
            self.store_fragment(write)?;
        }
        Ok(())
    }

    /// Returns the base URNs of all stored entities, one per id.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be read.
    fn get_entity_ids(&self) -> Result<Vec<Urn>>;

    /// Iterates over all stored base URNs, calling `callback` with each chunk of ids.
    ///
    /// The default implementation materializes every id first. Disk-backed stores override it to
    /// stream chunks directly off a read transaction.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be read, or propagates any error the callback returns.
    fn for_each_entity_id_chunk(&self, chunk_size: usize, callback: &mut dyn FnMut(Vec<Urn>) -> Result<()>) -> Result<()> {
        let ids = self.get_entity_ids()?;
        for chunk in ids.chunks(chunk_size) {
            callback(chunk.to_vec())?;
        }
        Ok(())
    }

    /// Assembles the stored fragments for one base id into their emit-units and scope.
    ///
    /// **Assembly consumes the id's fragments.** An in-memory store drains them, so calling this
    /// twice for one id yields the fragments once and an empty `units` vec after. That is what the
    /// only caller wants: the scan visits each id exactly once and destroys the store immediately
    /// afterwards, and it is what keeps a series run from deep-copying every stored record on the
    /// way out.
    ///
    /// When no data exists for the id, returns an empty `units` vec; the caller treats that as a
    /// missing entity.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be read or a stored fragment cannot be decoded.
    fn assemble_entity(&self, base_id: &Urn) -> Result<AssembledFragments>;

    /// Returns the number of distinct base entity ids currently stored.
    fn count(&self) -> usize;

    /// Returns the number of emit-units currently stored across every id.
    ///
    /// For a current-state store this equals [`count`](Self::count) (one unit per id); for a series
    /// store it is the total number of stored observations.
    fn unit_count(&self) -> usize;

    /// Removes all stored data.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store
    /// cannot be cleared.
    fn clear(&self) -> Result<()>;

    /// Releases all resources held by the store.
    ///
    /// # Errors
    ///
    /// Returns [`EntityStoreError`](crate::entity_store::error::EntityStoreError) when the store's
    /// resources cannot be released.
    fn destroy(&self) -> Result<()>;

    /// Creates a boxed clone of this store.
    fn clone_box(&self) -> Box<dyn EntityStore>;
}

impl Clone for Box<dyn EntityStore> {
    fn clone(&self) -> Box<dyn EntityStore> {
        self.clone_box()
    }
}

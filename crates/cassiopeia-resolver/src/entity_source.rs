use crate::{entity_assembly::AssemblyTiming, error::Result};
use cassiopeia_ir::assembled_entity::AssembledEntity;
use std::ops::ControlFlow;
use urn_rs::Urn;

/// Assembles stored fragments into complete entities and streams them out.
///
/// A source reads back everything a [`FragmentSink`](crate::fragment_sink::FragmentSink) stored
/// (source data, scope, and relationships) and combines it into [`AssembledEntity`] values. One base
/// id assembles into one or more emit-units: a current-state store yields a single joined unit, while
/// a series store yields one unit per observation.
///
/// Implementations must be thread-safe (`Send + Sync`): entities may be assembled concurrently from
/// many threads.
pub trait EntitySource: Send + Sync {
    /// Assembles a base id's stored fragments and relationships into its emit-units.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when the store lookup fails, the id has
    /// no recorded mapping, or a stored relationship key is not a legal NGSI-LD name.
    fn assemble(&self, base_id: &Urn) -> Result<Vec<AssembledEntity>>;

    /// Returns the base URNs of all stored entities.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when the entity store cannot be read.
    fn get_entity_ids(&self) -> Result<Vec<Urn>>;

    /// Returns the number of unique entity ids currently stored.
    ///
    /// The count reflects deduplication: fragments targeting the same id are merged into a single
    /// entity. This is the number of entities the writer ultimately emits.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when the entity store cannot be read.
    fn get_unique_entity_count(&self) -> Result<u64> {
        Ok(u64::try_from(self.get_entity_ids()?.len()).unwrap_or(u64::MAX))
    }

    /// Returns the number of emit-units the source streams downstream.
    ///
    /// For a current-state source this equals [`get_unique_entity_count`](Self::get_unique_entity_count)
    /// (one unit per id); for a series source it is the total number of observations the
    /// extractor, transformer, and validator process before the fold stage collapses them back to one
    /// entity per id.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when the entity store cannot be read.
    fn get_emitted_count(&self) -> Result<u64> {
        self.get_unique_entity_count()
    }

    /// Assembles every stored entity, handing each emit-unit to `emit` in contiguous-id order.
    ///
    /// Assembly is parallel per chunk (rayon); `emit` is invoked serially on the calling thread, so a
    /// consumer that back-pressures blocks only this thread and never starves the rayon pool. `emit`
    /// returns [`ControlFlow::Break`] when its downstream consumer is gone, which ends the scan early.
    /// Each base id's units are emitted contiguously, so a downstream fold can group by id while
    /// holding one id at a time. The caller drives the scan on its own thread and owns the progress
    /// and channel plumbing; this method spawns nothing.
    ///
    /// `batch_size` is a target in *emit-units*, not ids: an implementation sizes its id chunks so
    /// one chunk yields roughly that many units, which is what keeps an id carrying many units from
    /// materialising the whole store before anything is emitted.
    ///
    /// Returns the [`AssemblyTiming`] the scan accumulated, so the caller (which owns the stage this
    /// work is measured as) can attribute it without timing the scan from outside.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when the entity store scan fails.
    fn drive_assembly(&self, batch_size: usize, emit: &mut dyn FnMut(Result<AssembledEntity>) -> ControlFlow<()>) -> Result<AssemblyTiming>;

    /// Releases all resources held by the source.
    fn destroy(&self);
}

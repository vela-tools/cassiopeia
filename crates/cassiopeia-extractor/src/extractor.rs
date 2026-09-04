use crate::{dropped_geometries::DroppedGeometries, error::Result};
use cassiopeia_ir::{assembled_entity::AssembledEntity, entity::Entity, mapped::Mapped};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

/// Fills in an entity's NGSI-LD attribute values from its mapping(s) and source record(s).
///
/// Implementations must be thread-safe (`Send + Sync`): a batch of entities may be extracted
/// concurrently across many threads.
pub trait Extractor: Send + Sync {
    /// Extracts one assembled entity, resolving every attribute declared in each of its mappings
    /// against that mapping's own record and unioning the results.
    ///
    /// An attribute whose value the mapping's transformation refuses is dropped and recorded: a
    /// refused geometry conversion in `dropped`, text that reads as no date-time in `unreadable`.
    /// The entity itself is still returned, so a refusal costs an attribute, not a record.
    ///
    /// # Errors
    ///
    /// Returns [`ExtractionError`](crate::error::ExtractionError) when an attribute's template
    /// fails to evaluate, or when nested attribute declarations recurse past the guard depth.
    fn extract(&self, assembled: AssembledEntity, dropped: &DroppedGeometries, unreadable: &UnreadableTimestamps) -> Result<Mapped<Entity>>;

    /// Extracts a batch of assembled entities, sharing one of each refusal sink across them.
    ///
    /// The default implementation processes entities sequentially; an implementation can override
    /// it to parallelize the work.
    fn extract_batch(&self, entities: Vec<AssembledEntity>, dropped: &DroppedGeometries, unreadable: &UnreadableTimestamps) -> Vec<Result<Mapped<Entity>>> {
        entities.into_iter().map(|entity| self.extract(entity, dropped, unreadable)).collect()
    }
}

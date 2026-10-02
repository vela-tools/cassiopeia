use crate::{dropped_attributes::DroppedAttributes, error::Result};
use cassiopeia_ir::{assembled_entity::AssembledEntity, entity::Entity, mapped::Mapped};

/// Fills in an entity's NGSI-LD attribute values from its mapping(s) and source record(s).
///
/// Implementations must be thread-safe (`Send + Sync`): a batch of entities may be extracted
/// concurrently across many threads.
pub trait Extractor: Send + Sync {
    /// Extracts one assembled entity, resolving every attribute declared in each of its mappings
    /// against that mapping's own record and unioning the results.
    ///
    /// An attribute that cannot be built from the record is dropped and recorded in `dropped`: a
    /// refused geometry conversion, text that reads as no date-time, or a template that fails to
    /// render. The entity itself is still returned, so the loss costs an attribute, not a record.
    ///
    /// # Errors
    ///
    /// Returns [`ExtractionError`](crate::error::ExtractionError) when a list relationship's
    /// instance template fails to evaluate while its objects are regrouped, or when nested attribute
    /// declarations recurse past the guard depth.
    fn extract(&self, assembled: AssembledEntity, dropped: &DroppedAttributes) -> Result<Mapped<Entity>>;

    /// Extracts a batch of assembled entities, sharing one set of sinks across them.
    ///
    /// The default implementation processes entities sequentially; an implementation can override
    /// it to parallelize the work.
    fn extract_batch(&self, entities: Vec<AssembledEntity>, dropped: &DroppedAttributes) -> Vec<Result<Mapped<Entity>>> {
        entities.into_iter().map(|entity| self.extract(entity, dropped)).collect()
    }
}

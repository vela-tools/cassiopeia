use crate::error::Result;
use cassiopeia_ir::{entity::Entity, mapped::Mapped};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

/// Transforms an extracted entity into an NGSI-LD entity.
///
/// Implementations must be thread-safe (`Send + Sync`): a batch of entities may be transformed
/// concurrently across many threads.
pub trait Transformer: Send + Sync {
    /// Transforms a single extracted entity into an NGSI-LD entity.
    ///
    /// An `observedAt` that reads as no instant is dropped and recorded in `unreadable`, so the
    /// attribute still publishes and the run can say which qualifier it lost.
    ///
    /// # Errors
    ///
    /// Returns [`TransformationError`](crate::error::TransformationError) when assembling the
    /// NGSI-LD entity from its builder fails.
    fn transform(&self, entity: Mapped<Entity>, unreadable: &UnreadableTimestamps) -> Result<NgsiLdEntity>;

    /// Transforms a batch of entities, sharing one unreadable-timestamp sink across them.
    ///
    /// The default implementation processes entities sequentially; an implementation can override
    /// it to parallelize the work.
    fn transform_batch(&self, entities: Vec<Mapped<Entity>>, unreadable: &UnreadableTimestamps) -> Vec<Result<NgsiLdEntity>> {
        entities.into_iter().map(|entity| self.transform(entity, unreadable)).collect()
    }
}

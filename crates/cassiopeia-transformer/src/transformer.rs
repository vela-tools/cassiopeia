use crate::{error::Result, qualifier_losses::QualifierLosses};
use cassiopeia_ir::{entity::Entity, mapped::Mapped};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;

/// Transforms an extracted entity into an NGSI-LD entity.
///
/// Implementations must be thread-safe (`Send + Sync`): a batch of entities may be transformed
/// concurrently across many threads.
pub trait Transformer: Send + Sync {
    /// Transforms a single extracted entity into an NGSI-LD entity.
    ///
    /// An `observedAt` or `expiresAt` that reads as no instant, and a `unitCode` that is no UN/CEFACT
    /// code, is left off and recorded in `losses`, so the attribute still publishes and the run can
    /// say which qualifier it lost.
    ///
    /// # Errors
    ///
    /// Returns [`TransformationError`](crate::error::TransformationError) when assembling the
    /// NGSI-LD entity from its builder fails.
    fn transform(&self, entity: Mapped<Entity>, losses: &QualifierLosses) -> Result<NgsiLdEntity>;

    /// Transforms a batch of entities, sharing one set of qualifier-loss sinks across them.
    ///
    /// The default implementation processes entities sequentially; an implementation can override
    /// it to parallelize the work.
    fn transform_batch(&self, entities: Vec<Mapped<Entity>>, losses: &QualifierLosses) -> Vec<Result<NgsiLdEntity>> {
        entities.into_iter().map(|entity| self.transform(entity, losses)).collect()
    }
}

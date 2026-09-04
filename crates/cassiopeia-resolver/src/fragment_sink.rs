use crate::error::Result;
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};

/// Consumes fragments into storage, recording relationships.
///
/// A sink stores each fragment's source data and tracks parent-child relationships between entities,
/// allowing complete entities to be assembled from the stored data.
///
/// Implementations must be thread-safe (`Send + Sync`): fragments may be resolved concurrently from
/// many threads.
pub trait FragmentSink: Send + Sync {
    /// Resolves a single fragment, storing its data and recording any relationships.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`](crate::error::ResolverError) when a temporal qualifier cannot be
    /// applied or a store operation fails.
    fn resolve(&self, fragment: Mapped<Fragment>) -> Result<()>;

    /// Resolves a batch of fragments.
    ///
    /// The default implementation processes fragments sequentially. Implementations can override
    /// this to batch or parallelize the work.
    fn resolve_batch(&self, fragments: Vec<Mapped<Fragment>>) -> Vec<Result<()>> {
        fragments.into_iter().map(|fragment| self.resolve(fragment)).collect()
    }
}

/// How a resolver store performs best when a batch of fragments is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreWriteStrategy {
    /// Independent writes can run concurrently without transactional setup.
    Concurrent,
    /// Writes should be coalesced into one transaction for the whole batch.
    TransactionalBatch,
}

impl StoreWriteStrategy {
    /// Combines two stores' requirements, preserving transactional batching when either store needs
    /// it.
    #[must_use]
    pub const fn combine(self, other: StoreWriteStrategy) -> StoreWriteStrategy {
        match (self, other) {
            (StoreWriteStrategy::Concurrent, StoreWriteStrategy::Concurrent) => StoreWriteStrategy::Concurrent,
            (StoreWriteStrategy::Concurrent | StoreWriteStrategy::TransactionalBatch, StoreWriteStrategy::TransactionalBatch)
            | (StoreWriteStrategy::TransactionalBatch, StoreWriteStrategy::Concurrent) => StoreWriteStrategy::TransactionalBatch,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::store_write_strategy::StoreWriteStrategy;

    #[test]
    fn concurrent_stores_remain_concurrent() {
        assert_eq!(
            StoreWriteStrategy::Concurrent.combine(StoreWriteStrategy::Concurrent),
            StoreWriteStrategy::Concurrent
        );
    }

    #[test]
    fn either_transactional_store_requires_batching() {
        assert_eq!(
            StoreWriteStrategy::Concurrent.combine(StoreWriteStrategy::TransactionalBatch),
            StoreWriteStrategy::TransactionalBatch
        );
        assert_eq!(
            StoreWriteStrategy::TransactionalBatch.combine(StoreWriteStrategy::Concurrent),
            StoreWriteStrategy::TransactionalBatch
        );
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

/// The batch of resolver writes a fragment reached its store in.
///
/// The resolver writes one batch at a time: every write of one batch completes before any write of
/// the next, and the batches are numbered in the order they are written. Within a batch the writes
/// can reach a store in any order, so a store whose outcome depends on the order of an entity's
/// records keeps the records of a batch apart until a write of a later batch, or assembly, shows that
/// none of them is still to come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WriteBatch(u64);

/// Numbers the resolver's write batches in the order it writes them.
#[derive(Debug, Default)]
pub struct WriteBatches(AtomicU64);

impl WriteBatches {
    /// A counter that has numbered no batch yet.
    #[must_use]
    pub const fn new() -> WriteBatches {
        WriteBatches(AtomicU64::new(0))
    }

    /// The number of the next batch, later than every batch numbered before it.
    pub fn next(&self) -> WriteBatch {
        WriteBatch(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::write_batch::WriteBatches;

    #[test]
    fn each_batch_is_numbered_after_the_one_before_it() {
        let batches = WriteBatches::new();
        let first = batches.next();
        let second = batches.next();
        let third = batches.next();
        assert!(first < second);
        assert!(second < third);
    }
}

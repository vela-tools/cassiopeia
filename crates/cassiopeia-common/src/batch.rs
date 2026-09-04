//! The group of items one pipeline stage hands to the next as a single message.

use derive_more::{Deref, From, Into, IntoIterator};

/// A group of items moved between two pipeline stages as one message.
///
/// Every per-record handoff in the pipeline carries a `Batch` rather than a single item, so the
/// per-message costs a stage boundary imposes (the channel's own counters and clock reads, the
/// progress-bar update, the telemetry span) are paid once per batch instead of once per record.
/// The batch is a transport grouping only: it says nothing about how a stage processes the items,
/// and the order of the items inside it is the order they were produced in.
///
/// A batch may be empty. A producer that has nothing to hand over sends nothing rather than an empty
/// batch, but a stage that filters its input can legitimately produce one, and every consumer treats
/// it as "no items", never as end-of-stream: that is [`Signal::Stop`](crate::signal::Signal::Stop).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deref, From, Into, IntoIterator)]
#[deref(forward)]
#[into_iterator(owned, ref, ref_mut)]
pub struct Batch<T>(Vec<T>);

impl<T> Batch<T> {
    /// Creates an empty batch sized to hold `capacity` items without reallocating.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Batch<T> {
        Batch(Vec::with_capacity(capacity))
    }

    /// Appends one item to the batch.
    pub fn push(&mut self, item: T) {
        self.0.push(item);
    }

    /// Returns the number of items in the batch.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the batch holds no items.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the item count as a `u64`, saturating, for the progress and telemetry counters.
    #[must_use]
    pub fn count(&self) -> u64 {
        u64::try_from(self.0.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use crate::batch::Batch;
    use std::mem;

    #[test]
    fn a_batch_built_by_pushing_reports_its_length_and_count() {
        let mut batch = Batch::with_capacity(4);
        batch.push(1);
        batch.push(2);
        batch.push(3);

        assert_eq!(batch.len(), 3);
        assert_eq!(batch.count(), 3);
        assert!(!batch.is_empty());
    }

    #[test]
    fn an_empty_batch_is_empty_and_yields_nothing() {
        let batch: Batch<u8> = Batch::default();

        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.count(), 0);
        assert_eq!(batch.into_iter().count(), 0);
    }

    #[test]
    fn a_batch_iterates_in_the_order_its_items_were_pushed() {
        let batch = Batch::from(vec![10, 20, 30]);

        let owned: Vec<u32> = batch.clone().into_iter().collect();
        let borrowed: Vec<u32> = batch.iter().copied().collect();

        assert_eq!(owned, vec![10, 20, 30]);
        assert_eq!(borrowed, vec![10, 20, 30]);
    }

    #[test]
    fn a_batch_derefs_to_its_slice() {
        let batch = Batch::from(vec![1, 2, 3]);

        assert_eq!(batch.first(), Some(&1));
        assert_eq!(&batch[1..], &[2, 3]);
    }

    #[test]
    fn a_batch_round_trips_through_the_vector_it_wraps() {
        let source = vec!["a".to_string(), "b".to_string()];

        let round_tripped: Vec<String> = Batch::from(source.clone()).into();

        assert_eq!(round_tripped, source);
    }

    #[test]
    fn taking_a_batch_leaves_an_empty_one_behind() {
        let mut batch = Batch::from(vec![1, 2]);

        let taken = mem::take(&mut batch);

        assert_eq!(taken.len(), 2);
        assert!(batch.is_empty());
    }
}

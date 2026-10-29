use crate::{timestamp_loss::TimestampLoss, unreadable_timestamp::UnreadableTimestamp};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::IndexMap;
use parking_lot::Mutex;

/// One record in the sink: which timestamp of which attribute failed, and what it failed on.
///
/// The key pairs the loss with the attribute, so one attribute losing two different timestamps on
/// the same records (its own value and its `expiresAt`) is two records, each reported under the code
/// that says what its loss cost.
pub type LossRecord = ((TimestampLoss, NameBuf), UnreadableTimestamp);

/// The timestamps one batch could not read, one record per attribute timestamp that lost one.
///
/// The sink is shared, not owned per entity: a batch is processed across a Rayon pool, so every
/// worker records into the same one behind a shared reference.
///
/// The map is an [`IndexMap`] behind a [`Mutex`] rather than a `DashMap`: a run's messages come out
/// in the order the failures were met rather than in a hash order that would reshuffle between
/// builds, and a sink is built per batch and stays empty in almost every one, so a sharded map's
/// per-CPU allocation would be paid for nothing. The lock is only ever taken on the failure path.
#[derive(Default)]
pub struct UnreadableTimestamps {
    entries: Mutex<IndexMap<(TimestampLoss, NameBuf), UnreadableTimestamp>>,
}

impl UnreadableTimestamps {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnreadableTimestamps {
        UnreadableTimestamps::default()
    }

    /// Records that `attribute`'s `loss` timestamp carried `text`, which is not a timestamp.
    ///
    /// The first text an attribute fails on is kept as its example; every later one is counted
    /// against that same record rather than opening another. Which text that is, when a batch is
    /// processed in parallel and its records carry different ones, is whichever reached the sink
    /// first: the example illustrates the shape, it does not identify a record.
    pub fn record(&self, loss: TimestampLoss, attribute: &NameBuf, text: &str) {
        self.entries
            .lock()
            .entry((loss, attribute.clone()))
            .and_modify(UnreadableTimestamp::count_another)
            .or_insert_with(|| UnreadableTimestamp::first(text));
    }

    /// Whether nothing failed to read, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every attribute timestamp that was lost and what it lost, in the order they first failed.
    #[must_use]
    pub fn into_entries(self) -> Vec<LossRecord> {
        self.entries.into_inner().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{timestamp_loss::TimestampLoss, unreadable_timestamps::UnreadableTimestamps};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnreadableTimestamps::new().is_empty());
    }

    #[test]
    fn every_text_one_attribute_fails_on_is_counted_against_one_record() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::AttributeValue, &name("dateObserved"), "2026-03-01 11:04:35+00:00");
        sink.record(TimestampLoss::AttributeValue, &name("dateObserved"), "2026-03-01 11:04:36+00:00");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, (TimestampLoss::AttributeValue, name("dateObserved")));
        assert_eq!(entries[0].1.occurrences.get(), 2);
        assert_eq!(entries[0].1.example.as_ref(), "2026-03-01 11:04:35+00:00");
    }

    #[test]
    fn two_attributes_that_lose_a_timestamp_are_two_records_in_the_order_they_failed() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::ObservedAt, &name("dateObservedTo"), "later");
        sink.record(TimestampLoss::ObservedAt, &name("dateObserved"), "earlier");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.1, name("dateObservedTo"));
        assert_eq!(entries[1].0.1, name("dateObserved"));
    }

    #[test]
    fn one_attribute_losing_two_different_timestamps_is_two_records() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::ObservedAt, &name("temperature"), "not a timestamp");
        sink.record(TimestampLoss::ExpiresAt, &name("temperature"), "tomorrow");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, (TimestampLoss::ObservedAt, name("temperature")));
        assert_eq!(entries[0].1.example.as_ref(), "not a timestamp");
        assert_eq!(entries[1].0, (TimestampLoss::ExpiresAt, name("temperature")));
        assert_eq!(entries[1].1.example.as_ref(), "tomorrow");
    }

    #[test]
    fn the_same_attribute_and_loss_meet_again_in_one_record_not_two() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::ExpiresAt, &name("until"), "one");
        sink.record(TimestampLoss::ExpiresAt, &name("until"), "two");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.occurrences.get(), 2);
    }
}

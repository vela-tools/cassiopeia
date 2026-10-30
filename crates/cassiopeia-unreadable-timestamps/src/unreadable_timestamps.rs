use crate::{
    attribute_loss::{AttributeLoss, AttributeLossRef},
    timestamp_loss::TimestampLoss,
    unreadable_timestamp::UnreadableTimestamp,
};
use cassiopeia_batch_sink::batch_sink::BatchSink;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::{Deref, Into};

/// The attribute timestamps one batch could not read, one record per [`AttributeLoss`].
#[derive(Default, Deref, Into)]
pub struct UnreadableTimestamps(BatchSink<AttributeLoss, UnreadableTimestamp>);

impl UnreadableTimestamps {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnreadableTimestamps {
        UnreadableTimestamps::default()
    }

    /// Records that `attribute`'s `loss` timestamp carried `text`, which is not a timestamp.
    ///
    /// The first text an attribute fails on is kept as its example; every later one is counted
    /// against that same record.
    pub fn record(&self, loss: TimestampLoss, attribute: &NameBuf, text: &str) {
        let key = AttributeLossRef { loss, attribute };
        self.0.tally(&key, || (key.to_owned_key(), UnreadableTimestamp::first(text)));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_loss::AttributeLoss,
        timestamp_loss::TimestampLoss,
        unreadable_timestamp::UnreadableTimestamp,
        unreadable_timestamps::UnreadableTimestamps,
    };
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn loss(loss: TimestampLoss, attribute: &str) -> AttributeLoss {
        AttributeLoss {
            loss,
            attribute: name(attribute),
        }
    }

    fn entries(sink: UnreadableTimestamps) -> Vec<(AttributeLoss, UnreadableTimestamp)> {
        BatchSink::from(sink).into_entries().collect()
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

        let entries = entries(sink);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, loss(TimestampLoss::AttributeValue, "dateObserved"));
        assert_eq!(entries[0].1.occurrences().get(), 2);
        assert_eq!(entries[0].1.example.as_ref(), "2026-03-01 11:04:35+00:00");
    }

    #[test]
    fn two_attributes_that_lose_a_timestamp_are_two_records_in_the_order_they_failed() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::ObservedAt, &name("dateObservedTo"), "later");
        sink.record(TimestampLoss::ObservedAt, &name("dateObserved"), "earlier");

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.attribute, name("dateObservedTo"));
        assert_eq!(entries[1].0.attribute, name("dateObserved"));
    }

    #[test]
    fn one_attribute_losing_two_different_timestamps_is_two_records() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::ObservedAt, &name("temperature"), "not a timestamp");
        sink.record(TimestampLoss::ExpiresAt, &name("temperature"), "tomorrow");

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, loss(TimestampLoss::ObservedAt, "temperature"));
        assert_eq!(entries[0].1.example.as_ref(), "not a timestamp");
        assert_eq!(entries[1].0, loss(TimestampLoss::ExpiresAt, "temperature"));
        assert_eq!(entries[1].1.example.as_ref(), "tomorrow");
    }

    #[test]
    fn the_same_attribute_and_loss_meet_again_in_one_record_not_two() {
        let sink = UnreadableTimestamps::new();
        sink.record(TimestampLoss::ExpiresAt, &name("until"), "one");
        sink.record(TimestampLoss::ExpiresAt, &name("until"), "two");

        let entries = entries(sink);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.occurrences().get(), 2);
    }
}

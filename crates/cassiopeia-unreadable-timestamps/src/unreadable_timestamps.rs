use crate::unreadable_timestamp::UnreadableTimestamp;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::IndexMap;
use parking_lot::Mutex;

/// The timestamps one batch could not read, one record per attribute that lost one.
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
    entries: Mutex<IndexMap<NameBuf, UnreadableTimestamp>>,
}

impl UnreadableTimestamps {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnreadableTimestamps {
        UnreadableTimestamps::default()
    }

    /// Records that `attribute` carried `text`, which is not a timestamp.
    ///
    /// The first text an attribute fails on is kept as its example; every later one is counted
    /// against that same record rather than opening another. Which text that is, when a batch is
    /// processed in parallel and its records carry different ones, is whichever reached the sink
    /// first: the example illustrates the shape, it does not identify a record.
    pub fn record(&self, attribute: &NameBuf, text: &str) {
        self.entries
            .lock()
            .entry(attribute.clone())
            .and_modify(UnreadableTimestamp::count_another)
            .or_insert_with(|| UnreadableTimestamp::first(text));
    }

    /// Whether nothing failed to read, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every attribute that lost a timestamp and what it lost, in the order they first failed.
    #[must_use]
    pub fn into_entries(self) -> Vec<(NameBuf, UnreadableTimestamp)> {
        self.entries.into_inner().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::unreadable_timestamps::UnreadableTimestamps;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

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
        sink.record(&name("dateObserved"), "2026-03-01 11:04:35+00:00");
        sink.record(&name("dateObserved"), "2026-03-01 11:04:36+00:00");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.occurrences.get(), 2);
        assert_eq!(entries[0].1.example.as_ref(), "2026-03-01 11:04:35+00:00");
    }

    #[test]
    fn two_attributes_that_lose_a_timestamp_are_two_records_in_the_order_they_failed() {
        let sink = UnreadableTimestamps::new();
        sink.record(&name("dateObservedTo"), "later");
        sink.record(&name("dateObserved"), "earlier");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, name("dateObservedTo"));
        assert_eq!(entries[1].0, name("dateObserved"));
    }
}

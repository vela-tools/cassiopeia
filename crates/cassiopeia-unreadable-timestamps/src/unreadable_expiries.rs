use crate::unreadable_timestamp::UnreadableTimestamp;
use indexmap::IndexMap;
use parking_lot::Mutex;
use urn_rs::Urn;

/// One record in the sink: the entity whose expiry failed, and what it failed on.
pub type ExpiryRecord = (Urn, UnreadableTimestamp);

/// The entity `expiresAt` values one batch could not read, one record per entity that lost one.
///
/// The key is the entity alone: an entity expiry has no attribute name, so unlike the attribute
/// sink there is nothing to pair the loss with. The sink is shared, not owned per record: a batch is
/// processed across a Rayon pool, so every worker records into the same one behind a shared
/// reference.
///
/// The map is an [`IndexMap`] behind a [`Mutex`] rather than a `DashMap`: a run's messages come out
/// in the order the failures were met rather than in a hash order that would reshuffle between
/// builds, and a sink is built per batch and stays empty in almost every one, so a sharded map's
/// per-CPU allocation would be paid for nothing. The lock is only ever taken on the failure path.
#[derive(Default)]
pub struct UnreadableExpiries {
    entries: Mutex<IndexMap<Urn, UnreadableTimestamp>>,
}

impl UnreadableExpiries {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnreadableExpiries {
        UnreadableExpiries::default()
    }

    /// Records that `entity`'s `expiresAt` carried `text`, which is not a timestamp.
    ///
    /// The first text an entity fails on is kept as its example; every later one is counted against
    /// that same record rather than opening another. Which text that is, when a batch is processed in
    /// parallel and its records carry different ones, is whichever reached the sink first: the
    /// example illustrates the shape, it does not identify a record.
    pub fn record(&self, entity: &Urn, text: &str) {
        self.entries
            .lock()
            .entry(entity.clone())
            .and_modify(UnreadableTimestamp::count_another)
            .or_insert_with(|| UnreadableTimestamp::first(text));
    }

    /// Whether nothing failed to read, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every entity expiry that was lost and what it lost, in the order they first failed.
    #[must_use]
    pub fn into_entries(self) -> Vec<ExpiryRecord> {
        self.entries.into_inner().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::unreadable_expiries::UnreadableExpiries;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnreadableExpiries::new().is_empty());
    }

    #[test]
    fn every_text_one_entity_fails_on_is_counted_against_one_record() {
        let sink = UnreadableExpiries::new();
        sink.record(&urn("urn:ngsi-ld:Sensor:1"), "the day after tomorrow");
        sink.record(&urn("urn:ngsi-ld:Sensor:1"), "next week");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, urn("urn:ngsi-ld:Sensor:1"));
        assert_eq!(entries[0].1.occurrences.get(), 2);
        assert_eq!(entries[0].1.example.as_ref(), "the day after tomorrow");
    }

    #[test]
    fn two_entities_that_lose_an_expiry_are_two_records_in_the_order_they_failed() {
        let sink = UnreadableExpiries::new();
        sink.record(&urn("urn:ngsi-ld:Sensor:2"), "later");
        sink.record(&urn("urn:ngsi-ld:Sensor:1"), "earlier");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, urn("urn:ngsi-ld:Sensor:2"));
        assert_eq!(entries[1].0, urn("urn:ngsi-ld:Sensor:1"));
    }
}

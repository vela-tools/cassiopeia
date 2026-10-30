use crate::unreadable_expiry::UnreadableExpiry;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use indexmap::IndexMap;
use parking_lot::Mutex;
use urn_rs::Urn;

/// One record in the sink: the entity type whose expiries failed, and what they failed on.
pub type ExpiryRecord = (NameBuf, UnreadableExpiry);

/// The entity `expiresAt` values one batch could not read, one record per entity type that lost
/// one.
///
/// The key is the entity type, which is the mapping's data model: an entity expiry has no
/// attribute name to pair the loss with, and keying it on the entity would open a record for every
/// entity a broken mapping produces. The type keeps the sink bounded by the mappings in the run,
/// as the attribute sink is bounded by their attributes. The sink is shared, not owned per record:
/// a batch is processed across a Rayon pool, so every worker records into the same one behind a
/// shared reference.
///
/// The map is an [`IndexMap`] behind a [`Mutex`] rather than a `DashMap`: a run's messages come out
/// in the order the failures were met rather than in a hash order that would reshuffle between
/// builds, and a sink is built per batch and stays empty in almost every one, so a sharded map's
/// per-CPU allocation would be paid for nothing. The lock is only ever taken on the failure path.
#[derive(Default)]
pub struct UnreadableExpiries {
    entries: Mutex<IndexMap<NameBuf, UnreadableExpiry>>,
}

impl UnreadableExpiries {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> UnreadableExpiries {
        UnreadableExpiries::default()
    }

    /// Records that `entity`, of `entity_type`, carried `text` as its `expiresAt`, which is not a
    /// timestamp.
    ///
    /// The first entity and text a type fails on are kept as its exemplar and example; every later
    /// one is counted against that same record rather than opening another. Which they are, when a
    /// batch is processed in parallel, is whichever reached the sink first: they illustrate the
    /// failure, they do not identify a record.
    pub fn record(&self, entity_type: &NameBuf, entity: &Urn, text: &str) {
        let mut entries = self.entries.lock();
        if let Some(record) = entries.get_mut(entity_type) {
            record.count_another();
            return;
        }

        // A type seen for the first time is the only case that owns anything: the map key must own
        // the type name, which is only borrowed from the mapping.
        entries.insert(entity_type.clone(), UnreadableExpiry::first(entity, text));
    }

    /// Whether nothing failed to read, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every entity type that lost an expiry and what it lost, in the order they first failed.
    #[must_use]
    pub fn into_entries(self) -> Vec<ExpiryRecord> {
        self.entries.into_inner().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::unreadable_expiries::UnreadableExpiries;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(UnreadableExpiries::new().is_empty());
    }

    #[test]
    fn every_entity_of_one_type_is_counted_against_one_record() {
        let sink = UnreadableExpiries::new();
        sink.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:1"), "the day after tomorrow");
        sink.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:2"), "next week");
        sink.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:3"), "never");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, name("Sensor"));
        assert_eq!(entries[0].1.exemplar, urn("urn:ngsi-ld:Sensor:1"));
        assert_eq!(entries[0].1.unreadable.occurrences.get(), 3);
        assert_eq!(entries[0].1.unreadable.example.as_ref(), "the day after tomorrow");
    }

    #[test]
    fn two_types_that_lose_an_expiry_are_two_records_in_the_order_they_failed() {
        let sink = UnreadableExpiries::new();
        sink.record(&name("Station"), &urn("urn:ngsi-ld:Station:1"), "later");
        sink.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:1"), "earlier");

        let entries = sink.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, name("Station"));
        assert_eq!(entries[1].0, name("Sensor"));
    }
}

use crate::unreadable_expiry::UnreadableExpiry;
use cassiopeia_batch_sink::batch_sink::BatchSink;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use derive_more::{Deref, Into};
use urn_rs::Urn;

/// The entity `expiresAt` values one batch could not read, one record per entity type that lost
/// one.
///
/// The key is the entity type, which is the mapping's data model: an entity expiry has no
/// attribute name to pair the loss with, and keying it on the entity would open a record for every
/// entity a broken mapping produces. The type keeps the sink bounded by the mappings in the run,
/// as the attribute sink is bounded by their attributes.
#[derive(Default, Deref, Into)]
pub struct UnreadableExpiries(BatchSink<NameBuf, UnreadableExpiry>);

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
    /// one is counted against that same record.
    pub fn record(&self, entity_type: &NameBuf, entity: &Urn, text: &str) {
        // A type seen for the first time is the only case that owns anything: the map key must own
        // the type name, which is only borrowed from the mapping.
        self.0.tally(entity_type, || (entity_type.clone(), UnreadableExpiry::first(entity, text)));
    }
}

#[cfg(test)]
mod tests {
    use crate::{unreadable_expiries::UnreadableExpiries, unreadable_expiry::UnreadableExpiry};
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    fn entries(sink: UnreadableExpiries) -> Vec<(NameBuf, UnreadableExpiry)> {
        BatchSink::from(sink).into_entries().collect()
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

        let entries = entries(sink);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, name("Sensor"));
        assert_eq!(entries[0].1.exemplar, urn("urn:ngsi-ld:Sensor:1"));
        assert_eq!(entries[0].1.occurrences().get(), 3);
        assert_eq!(entries[0].1.unreadable.example.as_ref(), "the day after tomorrow");
    }

    #[test]
    fn two_types_that_lose_an_expiry_are_two_records_in_the_order_they_failed() {
        let sink = UnreadableExpiries::new();
        sink.record(&name("Station"), &urn("urn:ngsi-ld:Station:1"), "later");
        sink.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:1"), "earlier");

        let entries = entries(sink);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, name("Station"));
        assert_eq!(entries[1].0, name("Sensor"));
    }
}

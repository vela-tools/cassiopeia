use ahash::RandomState;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use urn_rs::Urn;

/// Each base id's merged entity `expiresAt`, shared by every clone of an in-memory entity store.
///
/// Every fragment of an id may declare its own expiry, and clause 4.5.1 of ETSI GS CIM 009 v1.9.1
/// gives `expiresAt` one instant per entity, so a later fragment extends the bound only forwards:
/// the greatest expiry wins and none is ever cleared by a fragment without one.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConcurrentExpiries(Arc<DashMap<Urn, DateTime<Utc>, RandomState>>);

impl ConcurrentExpiries {
    /// Merges one fragment's expiry into the instant held for `base_id`, keeping the later one.
    pub(crate) fn merge(&self, base_id: &Urn, expires_at: DateTime<Utc>) {
        // A repeated id is the common case: every later fragment of an id lands on a key that is
        // already present, so `get_mut` serves it without cloning a URN the map would discard, and
        // only the first fragment of an id pays for an owned key.
        match self.0.get_mut(base_id) {
            Some(mut held) => {
                if expires_at > *held {
                    *held = expires_at;
                }
            }
            None => {
                self.0.insert(base_id.clone(), expires_at);
            }
        }
    }

    /// The merged expiry held for `base_id`, or `None` when none of its fragments had one.
    pub(crate) fn get(&self, base_id: &Urn) -> Option<DateTime<Utc>> {
        // The store keeps the instant for later assemblies of the id, so the caller gets its own copy.
        self.0.get(base_id).map(|held| *held)
    }

    /// Forgets every held expiry.
    pub(crate) fn clear(&self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::concurrent_expiries::ConcurrentExpiries;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    fn instant(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("valid instant")
    }

    #[test]
    fn the_greatest_expiry_survives_whatever_order_the_fragments_arrive_in() {
        let expiries = ConcurrentExpiries::default();
        let id = urn("urn:ngsi-ld:Sensor:1");

        expiries.merge(&id, instant(100));
        expiries.merge(&id, instant(300));
        expiries.merge(&id, instant(200));

        assert_eq!(expiries.get(&id), Some(instant(300)));
    }

    #[test]
    fn an_id_without_a_fragment_carrying_an_expiry_holds_none() {
        let expiries = ConcurrentExpiries::default();

        assert_eq!(expiries.get(&urn("urn:ngsi-ld:Sensor:absent")), None);
    }
}

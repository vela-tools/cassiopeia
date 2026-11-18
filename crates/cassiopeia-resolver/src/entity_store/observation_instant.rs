use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The instant a record's `observedAt` names, as a current-state store ranks the record by it.
///
/// Held as whole seconds and nanoseconds since the Unix epoch, in that order, so the derived order
/// is the order of the instants, and the disk-backed store encodes it as two integers rather than as
/// text it would have to parse back for every batch that reads the slot. The nanoseconds are kept
/// whole: two instants a sub-second apart rank apart in either store. This is an internal store
/// encoding and is unrelated to any ingested data format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObservationInstant {
    /// Whole seconds since the Unix epoch.
    seconds: i64,
    /// Nanoseconds past `seconds`.
    nanoseconds: u32,
}

/// A conversion of one representation of an instant into another, which no derive expresses.
impl From<DateTime<Utc>> for ObservationInstant {
    fn from(instant: DateTime<Utc>) -> ObservationInstant {
        ObservationInstant {
            seconds: instant.timestamp(),
            nanoseconds: instant.timestamp_subsec_nanos(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::observation_instant::ObservationInstant;
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn instants_rank_in_the_order_of_time_down_to_the_nanosecond() {
        let earlier = ObservationInstant::from(utc("2026-03-01T10:59:59.999999999Z"));
        let later = ObservationInstant::from(utc("2026-03-01T11:00:00Z"));
        let a_nanosecond_later = ObservationInstant::from(utc("2026-03-01T11:00:00.000000001Z"));

        assert!(earlier < later);
        assert!(later < a_nanosecond_later);
    }

    #[test]
    fn instants_before_the_epoch_rank_before_it() {
        let before = ObservationInstant::from(Utc.timestamp_opt(-1, 500_000_000).single().unwrap());
        let epoch = ObservationInstant::from(Utc.timestamp_opt(0, 0).single().unwrap());

        assert!(before < epoch);
    }

    #[test]
    fn an_instant_survives_the_store_encoding() {
        let instant = ObservationInstant::from(utc("2026-03-01T11:00:00.123456789Z"));
        let encoded = rmp_serde::to_vec(&instant).unwrap();

        assert_eq!(rmp_serde::from_slice::<ObservationInstant>(&encoded).unwrap(), instant);
    }
}

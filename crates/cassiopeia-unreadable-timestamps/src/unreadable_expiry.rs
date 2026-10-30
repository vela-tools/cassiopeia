use crate::unreadable_timestamp::UnreadableTimestamp;
use urn_rs::Urn;

/// What one entity type lost to entity `expiresAt` text that is not a timestamp, over one batch.
///
/// The type is the failure: a mapping that reads its expiry from the wrong field or in the wrong
/// spelling loses it on every entity it produces, so keying the record on the entity would grow it
/// with the input. One entity is kept as an exemplar, so a reader has a real record to go and
/// inspect, beside the example text and the count of how far the loss spread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnreadableExpiry {
    /// The first entity of this type whose expiry would not read. It illustrates the failure, it
    /// does not identify every record that shared it.
    pub exemplar: Urn,
    /// The first text that would not read, and how many records of this type carried one.
    pub unreadable: UnreadableTimestamp,
}

impl UnreadableExpiry {
    /// Opens a record for one entity type, holding `exemplar` and `example` as its first failure.
    #[must_use]
    pub fn first(exemplar: &Urn, example: &str) -> UnreadableExpiry {
        UnreadableExpiry {
            // The sink outlives the borrowed entity id, so the exemplar must own it; only the first
            // failure of a type pays for this.
            exemplar: exemplar.clone(),
            unreadable: UnreadableTimestamp::first(example),
        }
    }

    /// Counts one more occurrence, keeping the exemplar and example already held.
    pub const fn count_another(&mut self) {
        self.unreadable.count_another();
    }
}

#[cfg(test)]
mod tests {
    use crate::unreadable_expiry::UnreadableExpiry;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    #[test]
    fn the_first_entity_and_text_are_kept_however_many_follow() {
        let mut record = UnreadableExpiry::first(&urn("urn:ngsi-ld:Sensor:1"), "tomorrow");
        record.count_another();
        record.count_another();

        assert_eq!(record.exemplar, urn("urn:ngsi-ld:Sensor:1"));
        assert_eq!(record.unreadable.example.as_ref(), "tomorrow");
        assert_eq!(record.unreadable.occurrences.get(), 3);
    }
}

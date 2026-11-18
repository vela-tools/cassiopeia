use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::types::TemporalValueType};

/// Which timestamp of an attribute was lost because it read as no instant.
///
/// The three losses look the same to a mapping author (a source spelling Cassiopeia does not read),
/// but they cost different things: an attribute's own value is dropped entirely, while a qualifier
/// loss still publishes the attribute, without the qualifier that failed. The sink keys its records
/// by this alongside the attribute, so one attribute losing two different timestamps is two losses
/// named separately, each under the code that says what it cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimestampLoss {
    /// The attribute's own value, so no trace of the attribute is emitted at all.
    AttributeValue,
    /// The attribute's `observedAt` qualifier, so the value is emitted with nothing anchoring it in
    /// time (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    ObservedAt,
    /// The attribute's `expiresAt` qualifier, so the value is emitted without the instant after
    /// which it ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22).
    ExpiresAt,
}

impl TimestampLoss {
    /// What an `observedAt` sub-attribute whose text reads as no instant costs, and the name the
    /// loss is filed under: `observed_at` names the sub-attribute, `attribute` the top-level
    /// attribute it qualifies, and `temporal_reading` the time value type its declaration converts
    /// its value to, if any.
    ///
    /// A declaration that converts its value to a time refuses the text the way any attribute value
    /// that will not read is refused, so the sub-attribute is dropped and filed under its own name.
    /// Any other declaration keeps the text as it is, and the qualifier then reads no instant from
    /// it, so the attribute is emitted without `observedAt` (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2)
    /// and the loss is filed under the attribute. The extraction and transformation stages meet
    /// these losses on the records they handle; this names them for a record those stages never
    /// see, such as one a current-state store discarded.
    #[must_use]
    pub const fn of_unreadable_observed_at<'a>(
        temporal_reading: Option<TemporalValueType>,
        observed_at: &'a NameBuf,
        attribute: &'a NameBuf,
    ) -> (TimestampLoss, &'a NameBuf) {
        match temporal_reading {
            Some(_) => (TimestampLoss::AttributeValue, observed_at),
            None => (TimestampLoss::ObservedAt, attribute),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::timestamp_loss::TimestampLoss;
    use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::types::TemporalValueType};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn an_observed_at_read_as_a_time_drops_the_sub_attribute_under_its_own_name() {
        let (observed_at, attribute) = (name("observedAt"), name("temperature"));

        assert_eq!(
            TimestampLoss::of_unreadable_observed_at(Some(TemporalValueType::DateTime), &observed_at, &attribute),
            (TimestampLoss::AttributeValue, &observed_at)
        );
    }

    #[test]
    fn an_observed_at_kept_as_written_leaves_the_attribute_without_its_qualifier() {
        let (observed_at, attribute) = (name("observedAt"), name("temperature"));

        assert_eq!(
            TimestampLoss::of_unreadable_observed_at(None, &observed_at, &attribute),
            (TimestampLoss::ObservedAt, &attribute)
        );
    }
}

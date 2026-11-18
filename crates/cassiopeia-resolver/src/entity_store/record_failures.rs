use crate::entity_store::record_timestamp::RecordTimestamp;
use cassiopeia_mapping::{observed_at::ObservedAt, observed_template::ObservedTemplateIndex};
use serde::{Deserialize, Serialize};

/// Why a record's `observedAt` under one template named no instant, when that is a loss the run
/// reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimestampFailure {
    /// The template could not be resolved for the record.
    Unresolvable,
    /// The template rendered this text, which reads as no instant, as a report quotes it.
    Unreadable(ObservedAt),
}

/// The losses a record's `observedAt` templates met, each beside the template that met it.
///
/// A record a current-state store keeps reaches the extractor, which reports these itself; a record
/// the store releases never does, so the store counts its losses as it releases it. Empty for almost
/// every record, so it holds nothing on the heap until a template fails.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordFailures(Vec<(ObservedTemplateIndex, TimestampFailure)>);

impl RecordFailures {
    /// Notes what the record's `observedAt` under `template` yielded, keeping it when it is a loss.
    pub fn note(&mut self, template: ObservedTemplateIndex, timestamp: RecordTimestamp) {
        match timestamp {
            RecordTimestamp::Unresolvable => self.0.push((template, TimestampFailure::Unresolvable)),
            RecordTimestamp::Unreadable(text) => self.0.push((template, TimestampFailure::Unreadable(text))),
            RecordTimestamp::Readable(_) | RecordTimestamp::Absent => {}
        }
    }

    /// Whether any of the record's templates could not be resolved for it.
    #[must_use]
    pub fn any_unresolvable(&self) -> bool {
        self.0.iter().any(|(_, failure)| *failure == TimestampFailure::Unresolvable)
    }

    /// The text each template that rendered unreadable text rendered, beside the template.
    pub fn into_unreadable(self) -> impl Iterator<Item = (ObservedTemplateIndex, ObservedAt)> {
        self.0.into_iter().filter_map(|(template, failure)| match failure {
            TimestampFailure::Unreadable(text) => Some((template, text)),
            TimestampFailure::Unresolvable => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{observation_instant::ObservationInstant, record_failures::RecordFailures, record_timestamp::RecordTimestamp};
    use cassiopeia_mapping::{observed_at::ObservedAt, observed_template::ObservedTemplateIndex};
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;

    fn template(index: usize) -> ObservedTemplateIndex {
        ObservedTemplateIndex::new(index)
    }

    #[test]
    fn a_readable_or_absent_timestamp_is_no_loss() {
        let mut failures = RecordFailures::default();
        failures.note(
            template(0),
            RecordTimestamp::Readable(ObservationInstant::from(Utc.timestamp_opt(0, 0).single().unwrap())),
        );
        failures.note(template(1), RecordTimestamp::Absent);

        assert_eq!(failures, RecordFailures::default());
    }

    #[test]
    fn a_failed_template_and_unreadable_text_are_kept_beside_their_template() {
        let mut failures = RecordFailures::default();
        failures.note(template(0), RecordTimestamp::Unreadable(ObservedAt::new("soon")));
        failures.note(template(1), RecordTimestamp::Unresolvable);

        assert!(failures.any_unresolvable());
        assert_eq!(failures.into_unreadable().collect::<Vec<_>>(), [(template(0), ObservedAt::new("soon"))]);
    }

    #[test]
    fn unreadable_text_alone_is_not_a_failed_template() {
        let mut failures = RecordFailures::default();
        failures.note(template(0), RecordTimestamp::Unreadable(ObservedAt::new("soon")));

        assert!(!failures.any_unresolvable());
    }

    #[test]
    fn failures_survive_the_store_encoding() {
        let mut failures = RecordFailures::default();
        failures.note(template(0), RecordTimestamp::Unreadable(ObservedAt::new("soon")));
        failures.note(template(1), RecordTimestamp::Unresolvable);
        let encoded = rmp_serde::to_vec(&failures).unwrap();

        assert_eq!(rmp_serde::from_slice::<RecordFailures>(&encoded).unwrap(), failures);
    }
}

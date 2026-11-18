use crate::entity_store::observation_instant::ObservationInstant;
use cassiopeia_mapping::observed_at::ObservedAt;
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// What a record's `observedAt` template yielded, which a current-state store ranks the record by.
///
/// Only an instant ranks: a record without one ranks below every record with one, whatever the
/// reason. The reasons are still kept apart, because a record the store discards for text that
/// reads as no instant, or for a failed template, is a loss the run has to report, while a record
/// without a timestamp is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordTimestamp {
    /// The record's `observedAt` names this instant.
    Readable(ObservationInstant),
    /// The mapping is not temporal, or the record carries no `observedAt`: its value is null or
    /// blank text.
    Absent,
    /// The record's `observedAt` is this text, which reads as no instant, as it would be quoted in a
    /// report.
    Unreadable(ObservedAt),
    /// The mapping's `observedAt` template could not be resolved for the record.
    Unresolvable,
}

impl RecordTimestamp {
    /// Reads the value an `observedAt` template rendered for a record.
    ///
    /// The value is read as an attribute's `observedAt` is read on the way out, by
    /// [`Value::try_parse_datetime`]: text in any spelling the `datetime` transformation reads, a
    /// number as a Unix epoch, each converted to UTC, so two spellings of one instant rank as one.
    /// What carries no instant is told apart by [`Value::reported_text`], the rule that decides
    /// whether an unreadable value is reported: null and blank text are a value the record does not
    /// carry, and anything else is unreadable and keeps the text a report quotes.
    #[must_use]
    pub fn read(rendered: JsonValue) -> RecordTimestamp {
        // A rendered `observedAt` is a scalar in practice; a null nested in an array or object is
        // left out of the value read here, as it is left out of the qualifier the output writes.
        let Some(value) = Value::from_json(rendered, &mut OmittedNulls::default()) else {
            return RecordTimestamp::Absent;
        };
        match value.try_parse_datetime() {
            Some(instant) => RecordTimestamp::Readable(ObservationInstant::from(instant)),
            None => value
                .reported_text()
                .map_or(RecordTimestamp::Absent, |text| RecordTimestamp::Unreadable(ObservedAt::new(text))),
        }
    }

    /// The instant the record ranks by, `None` when it ranks below every record with one.
    #[must_use]
    pub const fn instant(&self) -> Option<ObservationInstant> {
        match self {
            RecordTimestamp::Readable(instant) => Some(*instant),
            RecordTimestamp::Absent | RecordTimestamp::Unreadable(_) | RecordTimestamp::Unresolvable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{observation_instant::ObservationInstant, record_timestamp::RecordTimestamp};
    use cassiopeia_mapping::observed_at::ObservedAt;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn readable(text: &str) -> RecordTimestamp {
        RecordTimestamp::Readable(ObservationInstant::from(text.parse::<DateTime<Utc>>().unwrap()))
    }

    #[test]
    fn text_naming_an_instant_reads_as_that_instant_in_utc() {
        assert_eq!(RecordTimestamp::read(json!("2026-03-01T12:00:00+02:00")), readable("2026-03-01T10:00:00Z"));
    }

    #[test]
    fn a_number_reads_as_a_unix_epoch() {
        assert_eq!(RecordTimestamp::read(json!(1_000_000_000)), readable("2001-09-09T01:46:40Z"));
    }

    #[test]
    fn null_and_blank_text_are_absent() {
        for rendered in [json!(null), json!(""), json!(" \t")] {
            assert_eq!(RecordTimestamp::read(rendered.clone()), RecordTimestamp::Absent, "{rendered}");
        }
    }

    #[test]
    fn text_naming_no_instant_is_unreadable_and_kept_as_a_report_quotes_it() {
        assert_eq!(RecordTimestamp::read(json!("soon")), RecordTimestamp::Unreadable(ObservedAt::new("soon")));
        assert_eq!(RecordTimestamp::read(json!("  soon ")), RecordTimestamp::Unreadable(ObservedAt::new("soon")));
    }

    #[test]
    fn any_other_value_naming_no_instant_is_unreadable_and_kept_as_its_rendering() {
        for (rendered, text) in [(json!(true), "true"), (json!(2026), "2026"), (json!([]), "[]"), (json!({}), "{}")] {
            assert_eq!(
                RecordTimestamp::read(rendered.clone()),
                RecordTimestamp::Unreadable(ObservedAt::new(text)),
                "{rendered}"
            );
        }
    }

    #[test]
    fn only_a_readable_timestamp_ranks_by_an_instant() {
        assert!(readable("2026-03-01T10:00:00Z").instant().is_some());
        assert_eq!(RecordTimestamp::Absent.instant(), None);
        assert_eq!(RecordTimestamp::Unreadable(ObservedAt::new("soon")).instant(), None);
        assert_eq!(RecordTimestamp::Unresolvable.instant(), None);
    }
}

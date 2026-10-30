use cassiopeia_batch_sink::recurring::Recurring;
use cassiopeia_ngsi_ld::value::types::Value;
use std::{borrow::Cow, num::NonZeroU64};

/// What one attribute lost to text that is not a timestamp, over one batch.
///
/// Only the first offending text is kept. A column of malformed timestamps holds a *different* text
/// on every row (they are timestamps, after all), so keying the record on the text would grow it
/// with the input and hand the reporter a line per record. The attribute is the failure; the text is
/// one example of it, and the count says how far it spread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnreadableTimestamp {
    /// The first text this attribute carried that would not read as a timestamp, kept so a mapping
    /// author can see the shape their source actually emits. It is an example of the failure, not a
    /// record identifier.
    pub example: Box<str>,
    /// How many times this attribute carried one.
    pub occurrences: NonZeroU64,
}

impl UnreadableTimestamp {
    /// Opens a record for one attribute, holding `example` as the text that would not read.
    #[must_use]
    pub fn first(example: &str) -> UnreadableTimestamp {
        UnreadableTimestamp {
            example: Box::from(example),
            occurrences: NonZeroU64::MIN,
        }
    }
}

impl Recurring for UnreadableTimestamp {
    fn count_another(&mut self) {
        self.occurrences = self.occurrences.saturating_add(1);
    }

    fn occurrences(&self) -> NonZeroU64 {
        self.occurrences
    }
}

/// The text an unreadable `value` is recorded under, or `None` when the record does not carry one.
///
/// A null or blank value is one the record does not carry and is not recorded; text is quoted as
/// written, and any other value (a number too small to be an epoch, a boolean, an object) as its
/// rendering, so a mapping mistake is named rather than lost. Text is borrowed from the value: a
/// sink copies it only when it opens a record, so a repeat of a known failure allocates nothing.
#[must_use]
pub fn unreadable_text(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() { None } else { Some(Cow::Borrowed(text)) }
        }
        Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
            Some(Cow::Owned(value.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::unreadable_timestamp::{UnreadableTimestamp, unreadable_text};
    use cassiopeia_batch_sink::recurring::Recurring;
    use cassiopeia_ngsi_ld::value::types::Value;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn the_first_text_is_kept_as_the_example_however_many_follow() {
        let mut record = UnreadableTimestamp::first("2026-03-01 11:04:35+00:00");
        record.count_another();
        record.count_another();

        assert_eq!(record.example.as_ref(), "2026-03-01 11:04:35+00:00");
        assert_eq!(record.occurrences.get(), 3);
    }

    #[test]
    fn a_null_or_blank_value_is_absent_rather_than_unreadable() {
        assert_eq!(unreadable_text(&Value::from(json!(null))), None);
        assert_eq!(unreadable_text(&Value::from(json!("   "))), None);
        assert_eq!(unreadable_text(&Value::from(json!(""))), None);
    }

    #[test]
    fn text_is_recorded_as_written_and_any_other_shape_as_its_rendering() {
        assert_eq!(unreadable_text(&Value::from(json!("  tomorrow "))), Some("tomorrow".into()));
        assert_eq!(unreadable_text(&Value::from(json!(2026))), Some("2026".into()));
        assert_eq!(unreadable_text(&Value::from(json!(true))), Some("true".into()));
    }
}

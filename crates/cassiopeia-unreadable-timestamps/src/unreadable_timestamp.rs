use std::num::NonZeroU64;

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

    /// Counts one more occurrence, keeping the example already held.
    pub const fn count_another(&mut self) {
        self.occurrences = self.occurrences.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::unreadable_timestamp::UnreadableTimestamp;

    #[test]
    fn the_first_text_is_kept_as_the_example_however_many_follow() {
        let mut record = UnreadableTimestamp::first("2026-03-01 11:04:35+00:00");
        record.count_another();
        record.count_another();

        assert_eq!(record.example.as_ref(), "2026-03-01 11:04:35+00:00");
        assert_eq!(record.occurrences.get(), 3);
    }
}

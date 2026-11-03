use crate::recurring::Recurring;
use std::num::NonZeroU64;

/// A record that keeps the first text a failure was met with, and how many times it was met.
///
/// Only the first text is kept. A failure that is a column of bad source values holds a *different*
/// text on most records, so keying the record on the text would grow it with the input and hand the
/// reporter a line per record. The key names the failure; the text is one example of it, kept so a
/// mapping author can see the shape their source actually emits, and the count says how far it
/// spread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountedExample {
    /// The first text the failure was met with. It is an example of the failure, not a record
    /// identifier.
    pub example: Box<str>,
    /// How many times the failure was met.
    pub occurrences: NonZeroU64,
}

impl CountedExample {
    /// Opens a record for a failure met once, holding `example` as the text it was met with.
    #[must_use]
    pub fn first(example: &str) -> CountedExample {
        CountedExample {
            example: Box::from(example),
            occurrences: NonZeroU64::MIN,
        }
    }
}

impl Recurring for CountedExample {
    fn count_another(&mut self) {
        self.occurrences = self.occurrences.saturating_add(1);
    }

    fn occurrences(&self) -> NonZeroU64 {
        self.occurrences
    }
}

#[cfg(test)]
mod tests {
    use crate::{counted_example::CountedExample, recurring::Recurring};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_first_text_is_kept_as_the_example_however_many_follow() {
        let mut record = CountedExample::first("2026-03-01 11:04:35+00:00");
        record.count_another();
        record.count_another();

        assert_eq!(record.example.as_ref(), "2026-03-01 11:04:35+00:00");
        assert_eq!(record.occurrences().get(), 3);
    }
}

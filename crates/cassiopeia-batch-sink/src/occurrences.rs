use crate::recurring::Recurring;
use std::num::NonZeroU64;

/// A record that holds nothing but its count, for a failure whose key already says all there is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Occurrences(NonZeroU64);

impl Occurrences {
    /// Opens a record for a failure met once.
    #[must_use]
    pub const fn first() -> Occurrences {
        Occurrences(NonZeroU64::MIN)
    }
}

impl Recurring for Occurrences {
    fn count_another(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    fn occurrences(&self) -> NonZeroU64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{occurrences::Occurrences, recurring::Recurring};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_first_record_counts_one_and_each_repeat_one_more() {
        let mut record = Occurrences::first();
        assert_eq!(record.occurrences().get(), 1);

        record.count_another();
        record.count_another();
        assert_eq!(record.occurrences().get(), 3);
    }
}

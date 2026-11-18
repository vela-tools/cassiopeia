use std::num::NonZeroU64;

/// A record of one failure that counts how often the failure recurred.
pub trait Recurring {
    /// Counts `occurrences` more occurrences, keeping whatever example the record already holds.
    fn count_more(&mut self, occurrences: NonZeroU64);

    /// Counts one more occurrence, keeping whatever example the record already holds.
    fn count_another(&mut self) {
        self.count_more(NonZeroU64::MIN);
    }

    /// How many times the failure occurred.
    fn occurrences(&self) -> NonZeroU64;
}

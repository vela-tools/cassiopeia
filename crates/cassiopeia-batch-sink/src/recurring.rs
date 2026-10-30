use std::num::NonZeroU64;

/// A record of one failure that counts how often the failure recurred.
pub trait Recurring {
    /// Counts one more occurrence, keeping whatever example the record already holds.
    fn count_another(&mut self);

    /// How many times the failure occurred.
    fn occurrences(&self) -> NonZeroU64;
}

use derive_more::Display;
use serde::{Deserialize, Serialize};

/// The 1-based position of a single run within a pipeline schedule.
///
/// A newtype rather than a bare `u32` so a run index cannot be confused with a run *count*
/// ([`RunCount`]) or any other integer threaded through the scheduler and the
/// [`MetaSignal`](crate::signal::MetaSignal) it feeds. Serializes transparently as its underlying
/// number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, Deserialize)]
pub struct RunNumber(u32);

impl RunNumber {
    /// Wraps a 1-based run index.
    #[must_use]
    pub const fn new(index: u32) -> RunNumber {
        RunNumber(index)
    }

    /// The underlying 1-based index, for formatting and zero-padding width computation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The total number of runs a bounded schedule will perform.
///
/// A newtype rather than a bare `u32` so a run total cannot be confused with a run *index*
/// ([`RunNumber`]); an unbounded schedule carries `None` rather than a sentinel count. Serializes
/// transparently as its underlying number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, Deserialize)]
pub struct RunCount(u32);

impl RunCount {
    /// Wraps a run total.
    #[must_use]
    pub const fn new(total: u32) -> RunCount {
        RunCount(total)
    }

    /// The underlying total, for zero-padding width computation and schedule bound checks.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::run::{RunCount, RunNumber};

    #[test]
    fn a_run_number_exposes_and_displays_its_index() {
        assert_eq!(RunNumber::new(7).get(), 7);
        assert_eq!(RunNumber::new(42).to_string(), "42");
    }

    #[test]
    fn run_numbers_order_by_index() {
        assert!(RunNumber::new(1) < RunNumber::new(2));
    }

    #[test]
    fn a_run_count_exposes_and_displays_its_total() {
        assert_eq!(RunCount::new(5).get(), 5);
        assert_eq!(RunCount::new(100).to_string(), "100");
    }

    #[test]
    fn both_newtypes_serialize_transparently_as_numbers() {
        let number = serde_json::to_string(&RunNumber::new(3)).unwrap();
        let count = serde_json::to_string(&RunCount::new(9)).unwrap();

        assert_eq!(number, "3");
        assert_eq!(count, "9");
        assert_eq!(serde_json::from_str::<RunNumber>(&number).unwrap(), RunNumber::new(3));
        assert_eq!(serde_json::from_str::<RunCount>(&count).unwrap(), RunCount::new(9));
    }
}

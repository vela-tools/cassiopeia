use crate::error::WriterError;
use cassiopeia_diagnostic::code::broker_code::BrokerCode;
use indexmap::IndexMap;
use parking_lot::Mutex;

/// What one distinct delivery failure accounted for.
struct RecordedFailure {
    /// The first occurrence, kept whole so its own `source()` chain survives to the final report.
    first: WriterError,
    /// How many times a failure with this identity happened.
    occurrences: u64,
}

/// What the run's recorded delivery failures amount to.
pub struct FailureSummary {
    /// How many distinct failures the run recorded.
    pub distinct: usize,
    /// The most frequent of them.
    pub most_frequent: WriterError,
}

/// The distinct delivery failures a broker run recorded, and how often each happened.
///
/// The map is an [`IndexMap`] behind a [`Mutex`] rather than a sharded map, for the same reason the
/// extractor's refused-geometry sink is: the report a run prints has to come out in the same order
/// every time, and the lock is only ever taken on the failure path.
///
/// The key is the failure's code plus its rendered headline, because the underlying errors are not
/// comparable (`reqwest::Error` and `serde_json::Error` are neither `Hash` nor `Eq`) while the
/// first occurrence is kept unrendered so the final report can still walk its causes.
#[derive(Default)]
pub struct BrokerFailures {
    entries: Mutex<IndexMap<(BrokerCode, Box<str>), RecordedFailure>>,
}

impl BrokerFailures {
    /// Opens an empty record.
    #[must_use]
    pub fn new() -> BrokerFailures {
        BrokerFailures::default()
    }

    /// Records one failure, counting a repeat of one already seen.
    pub fn record(&self, code: BrokerCode, error: WriterError) {
        let key = (code, error.to_string().into_boxed_str());
        let mut entries = self.entries.lock();
        match entries.get_mut(&key) {
            Some(recorded) => recorded.occurrences = recorded.occurrences.saturating_add(1),
            None => {
                entries.insert(key, RecordedFailure { first: error, occurrences: 1 });
            }
        }
    }

    /// Whether nothing was recorded, which is the healthy case.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Empties the record, yielding how many distinct failures it held and the most frequent one.
    ///
    /// Ties are broken by insertion order, so the earliest of two equally frequent failures wins and
    /// the answer is the same on every run over the same source.
    #[must_use]
    pub fn drain(&self) -> Option<FailureSummary> {
        let mut entries = self.entries.lock();
        let distinct = entries.len();
        let taken: IndexMap<(BrokerCode, Box<str>), RecordedFailure> = entries.drain(..).collect();
        drop(entries);

        let most_frequent = taken.into_values().max_by_key(|recorded| recorded.occurrences)?.first;
        Some(FailureSummary { distinct, most_frequent })
    }
}

#[cfg(test)]
mod tests {
    use crate::{broker::broker_failures::BrokerFailures, error::WriterError};
    use cassiopeia_diagnostic::code::broker_code::BrokerCode;

    fn opaque(status: u16) -> WriterError {
        WriterError::BrokerOpaqueStatus { status, count: 10 }
    }

    #[test]
    fn a_fresh_record_holds_nothing() {
        assert!(BrokerFailures::new().is_empty());
        assert!(BrokerFailures::new().drain().is_none());
    }

    #[test]
    fn the_most_frequent_failure_is_the_one_reported() {
        let failures = BrokerFailures::new();
        failures.record(BrokerCode::BatchRejectedOpaque, opaque(502));
        failures.record(BrokerCode::BatchRejectedOpaque, opaque(503));
        failures.record(BrokerCode::BatchRejectedOpaque, opaque(503));

        let summary = failures.drain().expect("a recorded failure");

        assert_eq!(summary.distinct, 2);
        assert!(summary.most_frequent.to_string().contains("503"));
    }

    #[test]
    fn draining_empties_the_record() {
        let failures = BrokerFailures::new();
        failures.record(BrokerCode::BatchRejectedOpaque, opaque(502));

        let _ = failures.drain();

        assert!(failures.is_empty());
    }
}

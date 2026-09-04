//! Run-level counters aggregated across every stage of one pipeline run.

use std::sync::atomic::{AtomicU64, Ordering};

/// Run-level counters with explicit units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunCounters {
    pub input_records: u64,
    pub fragments_created: u64,
    pub unique_entities: u64,
    pub entities_written: u64,
    pub errors: u64,
    pub warnings: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

/// The atomic backing store the run mutates concurrently, snapshotted into [`RunCounters`].
#[derive(Debug, Default)]
pub(crate) struct AtomicRunCounters {
    pub(crate) input_records: AtomicU64,
    pub(crate) fragments_created: AtomicU64,
    pub(crate) unique_entities: AtomicU64,
    pub(crate) entities_written: AtomicU64,
    pub(crate) errors: AtomicU64,
    pub(crate) warnings: AtomicU64,
    pub(crate) bytes_read: AtomicU64,
    pub(crate) bytes_written: AtomicU64,
}

impl AtomicRunCounters {
    pub(crate) fn snapshot(&self) -> RunCounters {
        RunCounters {
            input_records: self.input_records.load(Ordering::Relaxed),
            fragments_created: self.fragments_created.load(Ordering::Relaxed),
            unique_entities: self.unique_entities.load(Ordering::Relaxed),
            entities_written: self.entities_written.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            warnings: self.warnings.load(Ordering::Relaxed),
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::telemetry::run_counters::AtomicRunCounters;
    use std::sync::atomic::Ordering;

    #[test]
    fn a_snapshot_reflects_every_recorded_counter() {
        let counters = AtomicRunCounters::default();
        counters.input_records.fetch_add(7, Ordering::Relaxed);
        counters.entities_written.fetch_add(3, Ordering::Relaxed);
        let snapshot = counters.snapshot();
        assert_eq!(snapshot.input_records, 7);
        assert_eq!(snapshot.entities_written, 3);
        assert_eq!(snapshot.warnings, 0);
    }
}

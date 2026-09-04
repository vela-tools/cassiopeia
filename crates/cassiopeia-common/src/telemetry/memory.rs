//! Whole-run process resident-memory usage: peak and average resident set size (RSS).
//!
//! RSS is a process-global figure, sampled over the run's lifetime by an external sampler that calls
//! [`MemoryUsage::record`]. It is deliberately not attributed to individual stages: the pipeline runs
//! its stages concurrently in one address space, so no stage "owns" a share of resident memory that
//! the operating system or allocator could report. This module therefore measures the pipeline's
//! footprint as a whole.

use std::sync::atomic::{AtomicU64, Ordering};

/// The resident-memory figures for a run, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemorySnapshot {
    /// The highest resident set size observed across all samples.
    pub peak_bytes: u64,
    /// The mean resident set size across all samples, or zero when nothing was sampled.
    pub average_bytes: u64,
}

/// Concurrent accumulator for the resident-memory samples taken across a run.
///
/// The peak and the inputs to the average are kept as plain atomics so any thread may record a
/// sample without locking; the mean is computed from the running total and count at snapshot time.
#[derive(Debug, Default)]
pub struct MemoryUsage {
    peak_bytes: AtomicU64,
    total_bytes: AtomicU64,
    samples: AtomicU64,
}

impl MemoryUsage {
    /// Records one resident-set-size sample, updating the peak and the average's running inputs.
    pub fn record(&self, resident_bytes: u64) {
        self.peak_bytes.fetch_max(resident_bytes, Ordering::Relaxed);
        self.total_bytes.fetch_add(resident_bytes, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Builds an immutable snapshot; the average is zero until at least one sample has been recorded.
    #[must_use]
    pub fn snapshot(&self) -> MemorySnapshot {
        let samples = self.samples.load(Ordering::Relaxed);
        let total = self.total_bytes.load(Ordering::Relaxed);
        MemorySnapshot {
            peak_bytes: self.peak_bytes.load(Ordering::Relaxed),
            average_bytes: total.checked_div(samples).unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::telemetry::memory::MemoryUsage;

    #[test]
    fn an_unsampled_accumulator_reports_zero_peak_and_average() {
        let snapshot = MemoryUsage::default().snapshot();
        assert_eq!(snapshot.peak_bytes, 0);
        assert_eq!(snapshot.average_bytes, 0);
    }

    #[test]
    fn the_peak_tracks_the_largest_sample_regardless_of_order() {
        let usage = MemoryUsage::default();
        usage.record(100);
        usage.record(900);
        usage.record(400);
        assert_eq!(usage.snapshot().peak_bytes, 900);
    }

    #[test]
    fn the_average_is_the_mean_of_the_samples() {
        let usage = MemoryUsage::default();
        usage.record(100);
        usage.record(200);
        usage.record(300);
        assert_eq!(usage.snapshot().average_bytes, 200);
    }
}

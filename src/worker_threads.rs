//! How many worker threads the run's parallel stages are given.

use gdt_cpus::CpuInfo;
use std::{num::NonZeroUsize, thread::available_parallelism};

/// Resolves the worker-thread count for the run's shared thread pool.
///
/// A configured value is honoured verbatim. With none configured the count is detected: on hybrid
/// hardware it is the performance-core count, and on homogeneous hardware every logical processor.
///
/// Filling a hybrid machine's efficiency cores costs far more than it returns. On a 12P + 4E part the
/// pipeline finishes in the same wall time on 12 workers as on 16 while burning about 15% less CPU:
/// the extra workers land on cores several times slower than the ones already busy, so they add
/// scheduling and cache traffic without shortening the critical path. Counting is all this uses the
/// topology for: no thread is pinned, which the detection crate's own documentation notes has no
/// effect on Apple Silicon anyway.
#[must_use]
pub fn worker_thread_count(configured: Option<NonZeroUsize>) -> usize {
    if let Some(configured) = configured {
        return configured.get();
    }
    detect_worker_threads().unwrap_or_else(fallback_parallelism)
}

/// The performance-core count on hybrid hardware, or `None` when the topology says nothing useful.
///
/// Detection failing is not an error worth surfacing: it only means the run falls back to the same
/// count it would have used before the topology was consulted at all.
fn detect_worker_threads() -> Option<usize> {
    let info = CpuInfo::detect().ok()?;
    if !info.is_hybrid() {
        return None;
    }
    // A hybrid part reporting no performance cores is nonsense; take the fallback rather than
    // building a pool with no workers in it.
    match info.num_performance_cores() {
        0 => None,
        cores => Some(cores),
    }
}

/// Every logical processor, or one when the platform will not say.
fn fallback_parallelism() -> usize {
    available_parallelism().map_or(1, NonZeroUsize::get)
}

#[cfg(test)]
mod tests {
    use crate::worker_threads::{fallback_parallelism, worker_thread_count};
    use std::num::NonZeroUsize;

    #[test]
    fn a_configured_count_is_used_verbatim() {
        assert_eq!(worker_thread_count(NonZeroUsize::new(3)), 3);
    }

    #[test]
    fn an_unconfigured_count_is_detected_and_is_always_usable() {
        let detected = worker_thread_count(None);

        // The detected count is hardware-dependent, so the contract under test is that it is a
        // pool size that can actually run work and never exceeds what the machine offers.
        assert!(detected >= 1);
        assert!(detected <= fallback_parallelism());
    }

    #[test]
    fn the_fallback_reports_at_least_one_worker() {
        assert!(fallback_parallelism() >= 1);
    }
}

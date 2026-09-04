//! Background sampler that feeds process resident-memory readings into the run telemetry.
//!
//! The operating system exposes only the *current* resident set size, so peak and average memory
//! cannot be recovered after a run ends: they have to be sampled while it runs. A dedicated OS
//! thread takes the samples rather than a Tokio task, so a saturated async runtime cannot starve the
//! measurement, and the sampler stops and joins that thread when it is dropped, bracketing the run.

use cassiopeia_common::telemetry::run::RunTelemetry;
use memory_stats::memory_stats;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// How often resident memory is sampled. It matches the terminal reporter's redraw cadence: fine
/// enough to catch a run's peak, coarse enough to add no measurable overhead.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(80);

/// A running resident-memory sampler. Dropping it stops sampling and joins the sampler thread, so the
/// final sample is recorded before the caller reads the telemetry snapshot.
pub(crate) struct MemorySampler {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MemorySampler {
    /// Starts sampling `telemetry`'s process resident memory until the returned sampler is dropped.
    pub(crate) fn start(telemetry: Arc<RunTelemetry>) -> MemorySampler {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn({
            let stop = Arc::clone(&stop);
            move || {
                // Sample before the first sleep so even a run shorter than one interval is measured.
                loop {
                    if let Some(usage) = memory_stats() {
                        telemetry.record_memory(u64::try_from(usage.physical_mem).unwrap_or(u64::MAX));
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(SAMPLE_INTERVAL);
                }
            }
        });
        MemorySampler { stop, handle: Some(handle) }
    }
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // The sampler thread only reads memory, so a join failure would mean it panicked; there
            // is nothing to recover in a destructor, and the run's result is independent of it.
            let _ = handle.join();
        }
    }
}

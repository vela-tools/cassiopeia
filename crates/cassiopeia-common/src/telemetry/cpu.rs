//! Whole-run process CPU time, read from the CPU clock rather than the wall clock.
//!
//! CPU time counts processor seconds actually consumed across all of the run's threads, so on a
//! parallel pipeline it can exceed the wall-clock elapsed time. Dividing the two yields the run's
//! CPU utilisation. The value is captured relative to a start handle so it measures only the run,
//! not CPU the process burned before the run began.

use cpu_time::ProcessTime;
use std::time::Duration;

/// The CPU time consumed by a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuSnapshot {
    /// Total user plus system CPU time across every thread the run used.
    pub cpu_time: Duration,
}

/// A handle that measures process CPU time elapsed since it was created.
///
/// Holds the process CPU clock reading taken at run start; [`CpuUsage::snapshot`] subtracts it from
/// the current reading. The start reading is optional so a platform that cannot report process CPU
/// time yields a zero snapshot rather than failing the run.
#[derive(Debug, Clone, Copy)]
pub struct CpuUsage {
    started: Option<ProcessTime>,
}

impl CpuUsage {
    /// Captures the process CPU clock at run start.
    #[must_use]
    pub fn new() -> CpuUsage {
        CpuUsage {
            started: ProcessTime::try_now().ok(),
        }
    }

    /// Snapshots the CPU time consumed since [`CpuUsage::new`], or zero when unmeasurable.
    #[must_use]
    pub fn snapshot(&self) -> CpuSnapshot {
        CpuSnapshot {
            cpu_time: self.started.and_then(|started| started.try_elapsed().ok()).unwrap_or_default(),
        }
    }
}

impl Default for CpuUsage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::telemetry::cpu::CpuUsage;
    use std::{hint::black_box, time::Duration};

    #[test]
    fn cpu_time_is_non_decreasing_and_accrues_under_load() {
        let usage = CpuUsage::new();
        let before = usage.snapshot().cpu_time;
        // Burn a measurable slice of CPU so the second reading must exceed the first.
        let mut total: u64 = 0;
        for value in 0..20_000_000u64 {
            total = total.wrapping_add(black_box(value));
        }
        black_box(total);
        let after = usage.snapshot().cpu_time;
        assert!(after >= before);
        assert!(after > Duration::ZERO);
    }
}

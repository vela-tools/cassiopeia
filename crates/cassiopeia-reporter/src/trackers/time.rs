//! Tracking execution time for progress stages.

use execution_time::ExecutionTime;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};

/// Standalone duration tracker for progress stages.
#[derive(Clone, Default)]
pub struct TimeTracker {
    execution_time: Arc<Mutex<Option<ExecutionTime>>>,
    frozen_duration: Arc<Mutex<Option<Duration>>>,
}

impl TimeTracker {
    /// Builds an idle time tracker with no execution time set.
    #[must_use]
    pub fn new() -> TimeTracker {
        TimeTracker::default()
    }

    /// Sets the current execution-time source, clearing any frozen duration.
    pub fn set_execution_time(&self, execution_time: ExecutionTime) {
        *self.execution_time.lock() = Some(execution_time);
        *self.frozen_duration.lock() = None;
    }

    /// Freezes the current elapsed time so `elapsed` stops advancing.
    pub fn freeze(&self) {
        let mut execution_time = self.execution_time.lock();
        if let Some(active) = execution_time.as_ref() {
            *self.frozen_duration.lock() = Some(active.get_duration());
        }
        *execution_time = None;
    }

    /// Returns the current elapsed duration, or the frozen value once frozen.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        if let Some(frozen) = *self.frozen_duration.lock() {
            return frozen;
        }

        self.execution_time.lock().as_ref().map_or(Duration::ZERO, ExecutionTime::get_duration)
    }
}

#[cfg(test)]
mod tests {
    use crate::trackers::time::TimeTracker;
    use execution_time::ExecutionTime;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn an_idle_tracker_reports_zero() {
        assert_eq!(TimeTracker::new().elapsed(), Duration::ZERO);
    }

    #[test]
    fn a_frozen_tracker_stops_advancing() {
        let tracker = TimeTracker::new();
        tracker.set_execution_time(ExecutionTime::start());
        tracker.freeze();
        let first = tracker.elapsed();
        sleep(Duration::from_millis(10));

        assert_eq!(tracker.elapsed(), first);
    }
}

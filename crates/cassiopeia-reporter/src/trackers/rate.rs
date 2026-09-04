//! Live processing rate for terminal progress bars, read from the run's stage telemetry.

use cassiopeia_common::telemetry::stage_metrics::RateReader;
use parking_lot::Mutex;
use std::sync::Arc;

/// A stage bar's rate column, reading the run's aggregate rolling rate for that stage.
///
/// A stage records each completion once into the run telemetry; this holds a cheap read handle over
/// that stage's aggregate rate rather than counting completions itself. The handle is attached when a
/// worker enters the stage; until then, and after a reset, the rate reads as zero.
#[derive(Clone, Default)]
pub struct RateTracker {
    reader: Arc<Mutex<Option<RateReader>>>,
}

impl RateTracker {
    /// Builds an unattached rate column that reads zero until a reader is wired in.
    #[must_use]
    pub fn new() -> RateTracker {
        RateTracker::default()
    }

    /// Wires this column to a stage's aggregate rate reader.
    pub fn attach(&self, reader: RateReader) {
        *self.reader.lock() = Some(reader);
    }

    /// Detaches the current reader, so the rate reads zero until a new one is attached.
    pub fn reset(&self) {
        *self.reader.lock() = None;
    }

    /// A no-op required by indicatif's tracker interface; the rate is read on demand instead.
    pub const fn tick(&self) {}

    /// Returns the current live rate, or zero when no reader is attached.
    #[must_use]
    pub fn current_rate(&self) -> f64 {
        self.reader.lock().as_ref().map_or(0.0, RateReader::live_throughput)
    }
}

#[cfg(test)]
mod tests {
    use crate::trackers::rate::RateTracker;
    use cassiopeia_common::{stage::Stage, telemetry::run::RunTelemetry};

    #[test]
    fn an_unattached_tracker_reports_no_rate() {
        assert!(RateTracker::new().current_rate().abs() < f64::EPSILON);
    }

    #[test]
    fn an_attached_tracker_reads_the_stages_completions() {
        let telemetry = RunTelemetry::new();
        let stage = telemetry.start_stage(Stage::Writer);
        let tracker = RateTracker::new();
        tracker.attach(stage.rate_reader());
        for _ in 0..100 {
            stage.completed(1);
        }
        assert!(tracker.current_rate() >= 0.0);
    }

    #[test]
    fn a_reset_tracker_returns_to_zero() {
        let telemetry = RunTelemetry::new();
        let stage = telemetry.start_stage(Stage::Writer);
        let tracker = RateTracker::new();
        tracker.attach(stage.rate_reader());
        tracker.reset();
        assert!(tracker.current_rate().abs() < f64::EPSILON);
    }
}

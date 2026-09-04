//! Live-rendered progress-bar columns.
//!
//! Each type adapts one of the reporter's trackers (or a shared counter) to
//! indicatif's [`ProgressTracker`], rendering a single column of a stage bar:
//! processing rate, warning count, free-form latency annotation, and elapsed
//! time.

use crate::trackers::{rate::RateTracker, time::TimeTracker};
use anstyle::Reset;
use cassiopeia_terminal_style::{palette::CAUTION, symbol::WARN_SYMBOL};
use indicatif::{ProgressState, style::ProgressTracker};
use parking_lot::Mutex;
use std::{
    fmt::Write,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

/// Renders the smoothed processing rate (items/second).
pub(crate) struct IndicatifRateTracker(pub(crate) RateTracker);

impl ProgressTracker for IndicatifRateTracker {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(IndicatifRateTracker(self.0.clone()))
    }
    fn tick(&mut self, _state: &ProgressState, _now: Instant) {
        self.0.tick();
    }
    fn reset(&mut self, _state: &ProgressState, _now: Instant) {
        self.0.reset();
    }
    fn write(&self, _state: &ProgressState, w: &mut dyn Write) {
        let _ = write!(w, "{:>9}", format_rate(self.0.current_rate()));
    }
}

/// Renders a processing rate with a magnitude suffix (`/s`, `k/s`, `M/s`).
fn format_rate(rate: f64) -> String {
    if rate >= 1_000_000.0 {
        format!("{:.2}M/s", rate / 1_000_000.0)
    } else if rate >= 1_000.0 {
        format!("{:.2}k/s", rate / 1_000.0)
    } else if rate >= 1.0 {
        format!("{rate:.1}/s")
    } else {
        "0/s".to_string()
    }
}

/// Renders the accumulated warning count for a stage.
pub(crate) struct IndicatifWarnTracker(pub(crate) Arc<AtomicU64>);

impl ProgressTracker for IndicatifWarnTracker {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(IndicatifWarnTracker(self.0.clone()))
    }
    fn tick(&mut self, _state: &ProgressState, _now: Instant) {}
    fn reset(&mut self, _state: &ProgressState, _now: Instant) {}
    fn write(&self, _state: &ProgressState, w: &mut dyn Write) {
        let count = self.0.load(Ordering::SeqCst);
        if count > 0 {
            let _ = write!(w, "{}{WARN_SYMBOL} {}{}", CAUTION.render(), count, Reset.render());
        }
    }
}

/// Renders a live free-form annotation published via `stage_set_message`.
pub(crate) struct IndicatifMessageTracker(pub(crate) Arc<Mutex<String>>);

impl ProgressTracker for IndicatifMessageTracker {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(IndicatifMessageTracker(self.0.clone()))
    }
    fn tick(&mut self, _state: &ProgressState, _now: Instant) {}
    fn reset(&mut self, _state: &ProgressState, _now: Instant) {}
    fn write(&self, _state: &ProgressState, w: &mut dyn Write) {
        let guard = self.0.lock();
        if !guard.is_empty() {
            let _ = write!(w, "{}", *guard);
        }
    }
}

/// Renders the elapsed time for a stage.
pub(crate) struct IndicatifTimeTracker(pub(crate) TimeTracker);

impl ProgressTracker for IndicatifTimeTracker {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(IndicatifTimeTracker(self.0.clone()))
    }
    fn tick(&mut self, _state: &ProgressState, _now: Instant) {}
    fn reset(&mut self, _state: &ProgressState, _now: Instant) {}
    fn write(&self, _state: &ProgressState, w: &mut dyn Write) {
        let elapsed = self.0.elapsed();
        let _ = write!(w, "{:>6}", format_elapsed(elapsed.as_secs(), elapsed.subsec_millis()));
    }
}

/// Renders an elapsed time as an adaptive `h/m/s` string tightening as the duration shrinks.
fn format_elapsed(secs: u64, millis: u32) -> String {
    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{hours}h {mins:02}m {s:02}s")
    } else if secs >= 60 {
        let mins = secs / 60;
        let s = secs % 60;
        format!("{mins}m {s:02}.{:01}s", millis / 100)
    } else if secs >= 10 {
        format!("{secs}.{:02}s", millis / 10)
    } else {
        format!("{secs}.{millis:03}s")
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::column::{format_elapsed, format_rate};

    #[test]
    fn rate_scales_by_magnitude() {
        assert_eq!(format_rate(0.0), "0/s");
        assert_eq!(format_rate(5.0), "5.0/s");
        assert_eq!(format_rate(2_500.0), "2.50k/s");
        assert_eq!(format_rate(3_000_000.0), "3.00M/s");
    }

    #[test]
    fn elapsed_tightens_as_the_duration_shrinks() {
        assert_eq!(format_elapsed(3661, 0), "1h 01m 01s");
        assert_eq!(format_elapsed(90, 500), "1m 30.5s");
        assert_eq!(format_elapsed(12, 340), "12.34s");
        assert_eq!(format_elapsed(3, 7), "3.007s");
    }
}

//! Per-stage shared state and the animation thread that drives it.

use crate::{
    animation::pulse::PulseAnimation,
    stage_id::StageLabel,
    trackers::{rate::RateTracker, time::TimeTracker},
};
use indicatif::ProgressBar;
use parking_lot::Mutex;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

/// Delay between animation frames for indeterminate stage bars.
pub(crate) const ANIMATION_INTERVAL_MS: u64 = 40;

/// Default number of ticks used to size a freshly created stage bar.
pub(crate) const BAR_WIDTH: u64 = 30;

/// Whether a freshly built stage is already running or still waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StageActivity {
    /// The stage has started and its animation should run.
    Running,
    /// The stage is pre-registered but not yet started.
    Waiting,
}

/// Shared state for the animation thread of a single stage.
pub(crate) struct SharedStageState {
    pub(crate) frame: AtomicU64,
    pub(crate) running: AtomicBool,
    pub(crate) real_count: AtomicU64,
    pub(crate) total_count: AtomicU64,
    pub(crate) active_workers: AtomicU64,
    pub(crate) is_determinate: AtomicBool,
    pub(crate) has_warning: AtomicBool,
    pub(crate) warn_count: Arc<AtomicU64>,
    /// Live free-form annotation published via `stage_set_message` (e.g. broker writer latency
    /// readout). Rendered by `IndicatifMessageTracker`.
    pub(crate) message: Arc<Mutex<String>>,
    pub(crate) rate_tracker: RateTracker,
    pub(crate) time_tracker: TimeTracker,
    pub(crate) animation: PulseAnimation,
}

impl SharedStageState {
    /// Builds fresh shared state for a stage in the given `activity`.
    pub(crate) fn new(activity: StageActivity) -> Arc<SharedStageState> {
        Arc::new(SharedStageState {
            frame: AtomicU64::new(0),
            running: AtomicBool::new(activity == StageActivity::Running),
            real_count: AtomicU64::new(0),
            total_count: AtomicU64::new(0),
            active_workers: AtomicU64::new(0),
            is_determinate: AtomicBool::new(false),
            has_warning: AtomicBool::new(false),
            warn_count: Arc::new(AtomicU64::new(0)),
            message: Arc::new(Mutex::new(String::new())),
            rate_tracker: RateTracker::new(),
            time_tracker: TimeTracker::new(),
            animation: PulseAnimation::default(),
        })
    }
}

/// Tracks the progress bar and shared state for one stage.
///
/// Cloning shares the bar and the state rather than duplicating them, which is what lets the
/// registry hand a running stage its own entry to work through.
#[derive(Clone)]
pub(crate) struct StageEntry {
    pub(crate) bar: ProgressBar,
    pub(crate) label: StageLabel,
    pub(crate) shared: Arc<SharedStageState>,
}

/// Spawns the animation thread for an indeterminate stage bar.
///
/// The thread advances the pulse animation and mirrors the running item count into the bar message
/// until `shared.running` is cleared.
pub(crate) fn spawn_animation(bar: ProgressBar, shared: Arc<SharedStageState>) {
    thread::spawn(move || {
        while shared.running.load(Ordering::SeqCst) {
            if !shared.is_determinate.load(Ordering::SeqCst) {
                let frame = shared.frame.fetch_add(1, Ordering::SeqCst);
                let pos = shared.animation.get_position(frame);
                bar.set_position(pos);

                let count = shared.real_count.load(Ordering::SeqCst);
                let total = shared.total_count.load(Ordering::SeqCst);
                bar.set_message(format!("{count:>8}/{total:<8}"));
            }
            thread::sleep(Duration::from_millis(ANIMATION_INTERVAL_MS));
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::stage::{SharedStageState, StageActivity};
    use std::sync::atomic::Ordering;

    #[test]
    fn a_running_stage_starts_marked_running() {
        let shared = SharedStageState::new(StageActivity::Running);
        assert!(shared.running.load(Ordering::SeqCst));
    }

    #[test]
    fn a_waiting_stage_starts_not_running() {
        let shared = SharedStageState::new(StageActivity::Waiting);
        assert!(!shared.running.load(Ordering::SeqCst));
    }
}

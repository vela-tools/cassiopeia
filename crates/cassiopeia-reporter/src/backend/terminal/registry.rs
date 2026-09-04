//! The terminal backend's ordered registry of stage entries.

use crate::{
    backend::terminal::{
        entry_handle::TerminalStageHandle,
        stage::{BAR_WIDTH, SharedStageState, StageActivity, StageEntry, spawn_animation},
        style,
    },
    reporter::{ProgressStage, StageOutput},
    stage_handle::{SilentStageHandle, StageHandle},
    stage_id::{StageId, StageLabel},
};
use execution_time::ExecutionTime;
use indexmap::IndexMap;
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::Mutex;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Steady-tick interval for animated bars.
const TICK_INTERVAL: Duration = Duration::from_millis(80);

/// The stage bars the terminal backend draws, keyed by stage id in display order.
///
/// The registry is the only id-keyed structure in the backend, and it is touched only when a stage
/// is registered, entered, or annotated, never while one is running. A running stage works through
/// the [`TerminalStageHandle`] the registry handed it, which owns its entry outright.
pub(crate) struct StageRegistry {
    multi_progress: MultiProgress,
    entries: Mutex<IndexMap<StageId, StageEntry>>,
    quiet: AtomicBool,
}

impl StageRegistry {
    /// Builds an empty registry drawing into `multi_progress`.
    pub(crate) fn new(multi_progress: MultiProgress) -> StageRegistry {
        StageRegistry {
            multi_progress,
            entries: Mutex::new(IndexMap::new()),
            quiet: AtomicBool::new(false),
        }
    }

    /// Sets whether stage bars are drawn at all.
    pub(crate) fn set_quiet(&self, output: StageOutput) {
        self.quiet.store(output == StageOutput::Quiet, Ordering::SeqCst);
    }

    /// Whether stage output is currently suppressed.
    pub(crate) fn is_quiet(&self) -> bool {
        self.quiet.load(Ordering::Relaxed)
    }

    /// Registers stages in display order, so a later-starting stage still appears in its declared
    /// position rather than at the bottom.
    pub(crate) fn pre_register(&self, stages: &[Box<dyn ProgressStage>]) {
        if self.is_quiet() {
            return;
        }
        let mut entries = self.entries.lock();
        for stage in stages {
            let id = stage.id();
            if entries.contains_key(&id) {
                continue;
            }

            let bar = self.multi_progress.add(ProgressBar::new(BAR_WIDTH));
            bar.set_style(style::waiting());
            bar.set_prefix(stage.label().as_str());
            bar.set_message("Waiting");
            bar.enable_steady_tick(TICK_INTERVAL);

            let shared = SharedStageState::new(StageActivity::Waiting);
            entries.insert(
                id,
                StageEntry {
                    bar,
                    label: stage.label(),
                    shared,
                },
            );
        }
    }

    /// Starts one worker on a stage and returns the handle it draws through for its lifetime.
    ///
    /// The entry is resolved and cloned once, under the map's lock; the returned handle holds the
    /// bar and shared state directly, so nothing the running stage does comes back through here.
    pub(crate) fn enter(&self, id: StageId, label: StageLabel, execution_time: ExecutionTime) -> Box<dyn StageHandle> {
        if self.is_quiet() {
            return Box::new(SilentStageHandle::new());
        }
        let mut entries = self.entries.lock();
        let entry = if let Some(existing) = entries.get(&id) {
            // A pre-registered or already-finished stage is re-activated by its first worker; later
            // workers join the one that is already running.
            if existing.shared.active_workers.fetch_add(1, Ordering::SeqCst) == 0 {
                Self::activate(existing, execution_time);
            }
            existing.clone()
        } else {
            let entry = self.build_running(label, execution_time);
            entries.insert(id, entry.clone());
            entry
        };
        drop(entries);

        Box::new(TerminalStageHandle::new(entry))
    }

    /// Publishes a live annotation on a stage, for a publisher that holds no guard of its own.
    pub(crate) fn set_message(&self, id: StageId, message: &str) {
        if let Some(entry) = self.entries.lock().get(&id) {
            let mut annotation = entry.shared.message.lock();
            if *annotation != message {
                annotation.clear();
                annotation.push_str(message);
            }
        }
    }

    /// Stops every bar's steady-tick thread, so no late redraw paints over appended report text.
    pub(crate) fn disable_steady_ticks(&self) {
        for entry in self.entries.lock().values() {
            entry.bar.disable_steady_tick();
        }
    }

    /// Whether any stage is registered, for tests asserting that quiet mode registers nothing.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Whether a stage's animation thread is still running, for lifecycle tests.
    #[cfg(test)]
    pub(crate) fn is_running(&self, id: StageId) -> bool {
        self.entries.lock().get(&id).is_some_and(|entry| entry.shared.running.load(Ordering::SeqCst))
    }

    /// Re-activates a pre-registered or finished stage, switching it to a running style.
    fn activate(entry: &StageEntry, execution_time: ExecutionTime) {
        entry.shared.running.store(false, Ordering::SeqCst);
        entry.shared.real_count.store(0, Ordering::SeqCst);
        entry.shared.total_count.store(0, Ordering::SeqCst);
        entry.shared.is_determinate.store(false, Ordering::SeqCst);
        entry.shared.has_warning.store(false, Ordering::SeqCst);
        entry.shared.warn_count.store(0, Ordering::SeqCst);
        entry.shared.rate_tracker.reset();
        entry.shared.time_tracker.set_execution_time(execution_time);

        entry.bar.set_style(style::indeterminate(
            entry.shared.time_tracker.clone(),
            entry.shared.rate_tracker.clone(),
            entry.shared.warn_count.clone(),
            entry.shared.message.clone(),
        ));
        entry.bar.set_message("      0/0      ");

        entry.shared.running.store(true, Ordering::SeqCst);
        spawn_animation(entry.bar.clone(), entry.shared.clone());
    }

    /// Builds and starts a brand-new running stage entry, appending it to preserve display order.
    fn build_running(&self, label: StageLabel, execution_time: ExecutionTime) -> StageEntry {
        let bar = self.multi_progress.add(ProgressBar::new(BAR_WIDTH));
        let shared = SharedStageState::new(StageActivity::Running);
        shared.active_workers.store(1, Ordering::SeqCst);
        shared.time_tracker.set_execution_time(execution_time);

        bar.set_style(style::indeterminate(
            shared.time_tracker.clone(),
            shared.rate_tracker.clone(),
            shared.warn_count.clone(),
            shared.message.clone(),
        ));
        bar.set_prefix(label.as_str());
        bar.set_message("      0/0      ");
        bar.enable_steady_tick(TICK_INTERVAL);

        spawn_animation(bar.clone(), shared.clone());
        StageEntry { bar, label, shared }
    }
}

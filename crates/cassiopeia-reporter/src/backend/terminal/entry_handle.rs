//! The running-stage view of one terminal stage entry.

use crate::{
    backend::terminal::{stage::StageEntry, style},
    stage_handle::StageHandle,
};
use cassiopeia_common::telemetry::stage_metrics::RateReader;
use cassiopeia_terminal_style::symbol::{SUCCESS_SYMBOL, WARN_SYMBOL};
use std::sync::atomic::Ordering;

/// One running stage's handle to the bar and shared state that draw it.
///
/// The registry resolves the stage's entry once, when the stage is entered, and hands the worker
/// this handle. Every later progress event therefore touches only this stage's own bar and atomics,
/// never the registry's map, and never the process-wide lock that guards it.
pub(crate) struct TerminalStageHandle {
    entry: StageEntry,
}

impl TerminalStageHandle {
    /// Wraps one already-resolved stage entry.
    pub(crate) const fn new(entry: StageEntry) -> TerminalStageHandle {
        TerminalStageHandle { entry }
    }

    /// Repaints the bar in the warning style, preserving its live trackers.
    fn paint_warning(&self) {
        self.entry.bar.set_style(style::warning(
            self.entry.shared.time_tracker.clone(),
            self.entry.shared.rate_tracker.clone(),
            self.entry.shared.warn_count.clone(),
            self.entry.shared.message.clone(),
        ));
    }
}

impl StageHandle for TerminalStageHandle {
    fn inc_by(&self, count: u64) {
        let shared = &self.entry.shared;
        shared.real_count.fetch_add(count, Ordering::Relaxed);

        // An indeterminate bar has no declared total, so its own count doubles as the total it
        // renders against; a determinate one advances against the length already declared.
        if shared.is_determinate.load(Ordering::Relaxed) {
            self.entry.bar.inc(count);
        } else {
            shared.total_count.fetch_add(count, Ordering::Relaxed);
        }
    }

    fn set_length(&self, length: u64) {
        let shared = &self.entry.shared;
        shared.is_determinate.store(true, Ordering::SeqCst);
        shared.total_count.store(length, Ordering::SeqCst);
        self.entry.bar.set_length(length);
        self.entry.bar.set_position(shared.real_count.load(Ordering::SeqCst));
        self.entry.bar.set_style(style::determinate(
            shared.time_tracker.clone(),
            shared.rate_tracker.clone(),
            shared.warn_count.clone(),
            shared.message.clone(),
        ));
    }

    fn warn(&self) {
        if !self.entry.shared.has_warning.swap(true, Ordering::SeqCst) {
            self.paint_warning();
        }
    }

    fn warn_inc_by(&self, count: u64) {
        self.entry.shared.warn_count.fetch_add(count, Ordering::Relaxed);
        if !self.entry.shared.has_warning.swap(true, Ordering::SeqCst) {
            self.paint_warning();
        }
    }

    fn attach_rate(&self, reader: RateReader) {
        self.entry.shared.rate_tracker.attach(reader);
    }

    fn finish(&self) {
        let shared = &self.entry.shared;
        // Several workers can share one logical stage; only the last one to leave finishes the bar.
        if shared.active_workers.fetch_sub(1, Ordering::SeqCst) != 1 {
            return;
        }
        shared.running.store(false, Ordering::SeqCst);
        shared.time_tracker.freeze();

        let count = shared.real_count.load(Ordering::SeqCst);
        let total = shared.total_count.load(Ordering::SeqCst);
        let final_total = if total > 0 { total } else { count };

        self.entry.bar.set_length(final_total);
        self.entry.bar.set_position(count);

        let time = shared.time_tracker.clone();
        let rate = shared.rate_tracker.clone();
        let warns = shared.warn_count.clone();
        let message = shared.message.clone();

        if shared.has_warning.load(Ordering::SeqCst) {
            self.entry.bar.set_style(style::finished_warning(time, rate, warns, message));
            self.entry.bar.set_prefix(WARN_SYMBOL);
        } else {
            self.entry.bar.set_style(style::finished(time, rate, warns, message));
            self.entry.bar.set_prefix(SUCCESS_SYMBOL);
        }

        self.entry.bar.abandon_with_message(self.entry.label.as_str());
    }
}

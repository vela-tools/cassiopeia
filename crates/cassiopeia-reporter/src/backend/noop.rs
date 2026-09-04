//! A reporter that discards everything, used as the global fallback before installation.

use crate::{
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageReporter},
};
use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{diagnostic::Diagnostic, reason::Reason};
use execution_time::ExecutionTime;

/// A reporter that silently discards every message, progress update, and stage event.
///
/// Backs [`crate::global::reporter`] before the real reporter is installed, so callers never need to
/// handle an uninitialized reporter.
#[derive(Debug, Default)]
pub struct NoopReporter;

impl NoopReporter {
    /// Builds a no-op reporter.
    #[must_use]
    pub const fn new() -> NoopReporter {
        NoopReporter
    }
}

impl MessageLog for NoopReporter {
    fn info(&self, _message: &str) {}
    fn success(&self, _message: &str) {}
    fn debug(&self, _message: &str) {}
    fn step(&self, _current: usize, _total: usize, _message: &str) {}
    fn raw_log(&self, _message: &str) {}
}

impl DiagnosticSink for NoopReporter {
    fn report(&self, _diagnostic: &Diagnostic) {}
}

impl RunSummary for NoopReporter {
    fn summary(&self, _snapshot: &TelemetrySnapshot, _reasons: &[Reason]) {}
}

impl ProgressReporter for NoopReporter {
    fn start_progress(&self, _message: &str) {}
    fn update_progress(&self, _message: &str) {}
    fn progress_set_length(&self, _length: u64) {}
    fn progress_inc(&self) {}
    fn stop_progress(&self) {}
}

impl StageReporter for NoopReporter {
    fn enter_stage(&self, _stage: Box<dyn ProgressStage>, _execution_time: ExecutionTime) -> StageGuard {
        StageGuard::silent()
    }
    fn pre_register_stages(&self, _stages: &[Box<dyn ProgressStage>]) {}
}

#[cfg(test)]
mod tests {
    use crate::{backend::noop::NoopReporter, reporter::MessageLog};

    #[test]
    fn a_noop_reporter_discards_messages_without_panicking() {
        let reporter = NoopReporter::new();
        reporter.info("ignored");
        reporter.debug("ignored");
    }
}

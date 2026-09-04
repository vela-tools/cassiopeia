use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{code::diagnostic_code::DiagnosticCode, diagnostic::Diagnostic, reason::Reason};
use cassiopeia_reporter::{
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageReporter},
};
use execution_time::ExecutionTime;
use std::sync::{Mutex, PoisonError};

/// One diagnostic as a test sees it: what it was called, what it said, and how much it stood for.
#[derive(Clone, Debug)]
pub(crate) struct RecordedDiagnostic {
    /// The code the diagnostic carried.
    pub(crate) code: DiagnosticCode,
    /// The headline it rendered.
    pub(crate) headline: String,
    /// How many occurrences it stood for.
    pub(crate) occurrences: u64,
}

/// A reporter that records the diagnostics sent to it and discards everything else.
///
/// `const`-constructible so a test can back a `static` with it, which is what the stage spawners'
/// `&'static dyn Reporter` requires.
pub(crate) struct RecordingReporter {
    diagnostics: Mutex<Vec<RecordedDiagnostic>>,
}

impl RecordingReporter {
    /// Builds an empty reporter.
    pub(crate) const fn new() -> RecordingReporter {
        RecordingReporter {
            diagnostics: Mutex::new(Vec::new()),
        }
    }

    /// The diagnostics recorded so far.
    pub(crate) fn diagnostics(&self) -> Vec<RecordedDiagnostic> {
        self.diagnostics.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl MessageLog for RecordingReporter {
    fn info(&self, _message: &str) {}
    fn success(&self, _message: &str) {}
    fn debug(&self, _message: &str) {}
    fn step(&self, _current: usize, _total: usize, _message: &str) {}
    fn raw_log(&self, _message: &str) {}
}

impl DiagnosticSink for RecordingReporter {
    fn report(&self, diagnostic: &Diagnostic) {
        self.diagnostics.lock().unwrap_or_else(PoisonError::into_inner).push(RecordedDiagnostic {
            code: diagnostic.code(),
            headline: diagnostic.headline().to_string(),
            occurrences: diagnostic.occurrences().get(),
        });
    }
}

impl RunSummary for RecordingReporter {
    fn summary(&self, _snapshot: &TelemetrySnapshot, _reasons: &[Reason]) {}
}

impl ProgressReporter for RecordingReporter {
    fn start_progress(&self, _message: &str) {}
    fn update_progress(&self, _message: &str) {}
    fn progress_set_length(&self, _length: u64) {}
    fn progress_inc(&self) {}
    fn stop_progress(&self) {}
}

impl StageReporter for RecordingReporter {
    fn enter_stage(&self, _stage: Box<dyn ProgressStage>, _execution_time: ExecutionTime) -> StageGuard {
        StageGuard::silent()
    }
    fn pre_register_stages(&self, _stages: &[Box<dyn ProgressStage>]) {}
}

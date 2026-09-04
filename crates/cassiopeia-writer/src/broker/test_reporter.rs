use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{code::diagnostic_code::DiagnosticCode, diagnostic::Diagnostic, reason::Reason};
use cassiopeia_reporter::{
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageReporter},
    stage_id::StageId,
};
use execution_time::ExecutionTime;
use std::sync::{Mutex, PoisonError, atomic::AtomicBool};

/// One diagnostic as a test sees it: what it was called, and what it said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecordedDiagnostic {
    /// The code the diagnostic carried.
    pub(crate) code: DiagnosticCode,
    /// The headline it rendered.
    pub(crate) headline: String,
    /// The shortest explanation it offered.
    pub(crate) explanation: String,
}

/// A reporter that records the diagnostics and stage messages sent to it, for assertions.
pub(crate) struct TestReporter {
    diagnostics: Mutex<Vec<RecordedDiagnostic>>,
    messages: Mutex<Vec<(String, String)>>,
}

impl TestReporter {
    /// Builds an empty reporter. `const` so it can back a `static` in tests needing `&'static`.
    pub(crate) const fn new() -> TestReporter {
        TestReporter {
            diagnostics: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        }
    }

    /// The diagnostics recorded so far.
    pub(crate) fn diagnostics(&self) -> Vec<RecordedDiagnostic> {
        self.diagnostics.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The `(stage_id, message)` pairs recorded so far.
    pub(crate) fn messages(&self) -> Vec<(String, String)> {
        self.messages.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl MessageLog for TestReporter {
    fn info(&self, _message: &str) {}
    fn success(&self, _message: &str) {}
    fn debug(&self, _message: &str) {}
    fn step(&self, _current: usize, _total: usize, _message: &str) {}
    fn raw_log(&self, _message: &str) {}
}

impl DiagnosticSink for TestReporter {
    fn report(&self, diagnostic: &Diagnostic) {
        self.diagnostics.lock().unwrap_or_else(PoisonError::into_inner).push(RecordedDiagnostic {
            code: diagnostic.code(),
            headline: diagnostic.headline().to_string(),
            explanation: diagnostic.explanation().to_string(),
        });
    }
}

impl RunSummary for TestReporter {
    fn summary(&self, _snapshot: &TelemetrySnapshot, _reasons: &[Reason]) {}
}

impl ProgressReporter for TestReporter {
    fn start_progress(&self, _message: &str) {}
    fn update_progress(&self, _message: &str) {}
    fn progress_set_length(&self, _length: u64) {}
    fn progress_inc(&self) {}
    fn stop_progress(&self) {}
}

impl StageReporter for TestReporter {
    fn enter_stage(&self, _stage: Box<dyn ProgressStage>, _execution_time: ExecutionTime) -> StageGuard {
        StageGuard::silent()
    }
    fn pre_register_stages(&self, _stages: &[Box<dyn ProgressStage>]) {}
    fn stage_set_message(&self, id: StageId, message: &str) {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id.as_str().to_string(), message.to_string()));
    }
}

/// A process-lifetime shutdown flag for tests that need a `&'static AtomicBool`.
pub(crate) fn static_shutdown() -> &'static AtomicBool {
    static FLAG: AtomicBool = AtomicBool::new(false);
    &FLAG
}

/// A process-lifetime reporter for tests that need a `&'static dyn Reporter` but do not inspect it.
pub(crate) fn static_test_reporter() -> &'static TestReporter {
    static REPORTER: TestReporter = TestReporter::new();
    &REPORTER
}

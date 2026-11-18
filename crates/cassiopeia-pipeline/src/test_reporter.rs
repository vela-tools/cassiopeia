use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{code::diagnostic_code::DiagnosticCode, context_field::ContextField, diagnostic::Diagnostic, reason::Reason, verbosity::Verbosity};
use cassiopeia_reporter::{
    backend::terminal::diagnostic_lines::diagnostic_lines,
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageReporter},
};
use cassiopeia_terminal_style::rendering::Rendering;
use execution_time::ExecutionTime;
use std::sync::{Mutex, PoisonError};

/// One diagnostic as a test sees it: what it was called, what it said, and how much it stood for.
#[derive(Clone, Debug)]
pub(crate) struct RecordedDiagnostic {
    /// The code the diagnostic carried.
    pub(crate) code: DiagnosticCode,
    /// The headline it rendered.
    pub(crate) headline: String,
    /// Its rendered cause chain, outermost first.
    pub(crate) causes: Vec<String>,
    /// How many occurrences it stood for.
    pub(crate) occurrences: u64,
    /// The context that distinguishes it from another failure.
    pub(crate) defining: Vec<ContextField>,
    /// The context that describes one occurrence of it.
    pub(crate) incidental: Vec<ContextField>,
    /// The lines a terminal prints for it in full verbosity, without colour: the headline, the cause
    /// chain and every context field.
    pub(crate) rendered: Vec<String>,
}

/// A reporter that records the diagnostics and the info and success lines sent to it, and discards
/// everything else.
///
/// `const`-constructible so a test can back a `static` with it, which is what the stage spawners'
/// `&'static dyn Reporter` requires.
pub(crate) struct RecordingReporter {
    diagnostics: Mutex<Vec<RecordedDiagnostic>>,
    messages: Mutex<Vec<String>>,
}

impl RecordingReporter {
    /// Builds an empty reporter.
    pub(crate) const fn new() -> RecordingReporter {
        RecordingReporter {
            diagnostics: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        }
    }

    /// The diagnostics recorded so far.
    pub(crate) fn diagnostics(&self) -> Vec<RecordedDiagnostic> {
        self.diagnostics.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The info and success lines recorded so far, such as the line that reports a cycle, in the
    /// order they were sent.
    pub(crate) fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Records one info or success line.
    fn record(&self, message: &str) {
        self.messages.lock().unwrap_or_else(PoisonError::into_inner).push(message.to_owned());
    }
}

impl MessageLog for RecordingReporter {
    fn info(&self, message: &str) {
        self.record(message);
    }
    fn success(&self, message: &str) {
        self.record(message);
    }
    fn debug(&self, _message: &str) {}
    fn step(&self, _current: usize, _total: usize, _message: &str) {}
    fn raw_log(&self, _message: &str) {}
}

impl DiagnosticSink for RecordingReporter {
    fn report(&self, diagnostic: &Diagnostic) {
        self.diagnostics.lock().unwrap_or_else(PoisonError::into_inner).push(RecordedDiagnostic {
            code: diagnostic.code(),
            headline: diagnostic.headline().to_string(),
            causes: diagnostic.causes().iter().map(|cause| cause.as_str().to_string()).collect(),
            occurrences: diagnostic.occurrences().get(),
            // The diagnostic is only borrowed for the call, so its context is copied to outlive it.
            defining: diagnostic.defining().to_vec(),
            incidental: diagnostic.incidental().clone(),
            rendered: diagnostic_lines(diagnostic, Verbosity::Full, Rendering::Plain),
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

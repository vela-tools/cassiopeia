//! Terminal reporter backed by indicatif progress bars.

use crate::{
    backend::terminal::{diagnostic_lines::diagnostic_lines, registry::StageRegistry, style, summary::run_summary},
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageOutput, StageReporter},
    stage_id::StageId,
};
use anstyle::Style;
use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{diagnostic::Diagnostic, reason::Reason, verbosity::Verbosity};
use cassiopeia_terminal_style::{
    paint::paint,
    palette::{ACCENT, FRAME, SUCCESS},
    rendering::{Rendering, Stream, detect_rendering},
    symbol::{INFO_SYMBOL, SUCCESS_SYMBOL},
};
use execution_time::ExecutionTime;
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::Mutex;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Steady-tick interval for the one-off progress indicator.
const TICK_INTERVAL: Duration = Duration::from_millis(80);

/// Terminal reporter with indicatif progress bars.
///
/// The reporter itself owns only the message lines, the diagnostics, and the one-off progress
/// indicator; the multi-stage display lives in its [`StageRegistry`].
///
/// Both the styling and the verbosity are fixed at construction. Nothing downstream threads a level
/// through a call graph or consults a global to decide how much to print.
pub struct TerminalReporter {
    multi_progress: MultiProgress,
    stages: StageRegistry,
    progress_bar: Mutex<Option<ProgressBar>>,
    has_spacer: AtomicBool,
    rendering: Rendering,
    verbosity: Verbosity,
}

impl TerminalReporter {
    /// Builds a terminal reporter with no active stages or progress bar.
    ///
    /// `indicatif` hides its managed bars whenever stderr is not a terminal or the terminal is
    /// `dumb`, which is exactly when its output would otherwise vanish. In that case the reporter
    /// writes plain lines straight to stderr instead, so a piped run keeps its diagnostics, and it
    /// renders without ANSI codes so a redirected log stays greppable.
    #[must_use]
    pub fn new(verbosity: Verbosity) -> TerminalReporter {
        let multi_progress = MultiProgress::new();
        let rendering = if multi_progress.is_hidden() {
            Rendering::Plain
        } else {
            detect_rendering(Stream::Stderr)
        };
        TerminalReporter {
            stages: StageRegistry::new(multi_progress.clone()),
            multi_progress,
            progress_bar: Mutex::new(None),
            has_spacer: AtomicBool::new(false),
            rendering,
            verbosity,
        }
    }

    /// Writes one finished line, as a spent bar when the display is live and straight to stderr when
    /// it is hidden.
    fn emit(&self, line: &str) {
        if self.multi_progress.is_hidden() {
            eprintln!("{line}");
            return;
        }

        let bar = self.multi_progress.add(ProgressBar::new(0));
        if line.is_empty() {
            bar.set_style(style::spacer());
            bar.set_message(" ");
        } else {
            bar.set_style(style::message());
            bar.set_message(line.to_owned());
        }
        bar.finish();
    }

    /// Inserts the blank line that separates the first message from the stage bars above it.
    fn ensure_spacer(&self) {
        if self.multi_progress.is_hidden() || self.has_spacer.swap(true, Ordering::SeqCst) {
            return;
        }
        let spacer = self.multi_progress.add(ProgressBar::new(0));
        spacer.set_style(style::spacer());
        spacer.finish();
    }

    /// Prints a single symbol-prefixed message line.
    fn print_message(&self, symbol: &str, color: Style, message: &str) {
        self.ensure_spacer();
        if symbol.is_empty() {
            self.emit(message);
        } else {
            self.emit(&format!("{} {message}", paint(color, symbol, self.rendering)));
        }
    }
}

impl MessageLog for TerminalReporter {
    fn info(&self, message: &str) {
        self.print_message(INFO_SYMBOL, ACCENT, message);
    }
    fn success(&self, message: &str) {
        self.print_message(SUCCESS_SYMBOL, SUCCESS, message);
    }
    fn debug(&self, message: &str) {
        // A debug line is background detail: it earns a line only once the reader asked for depth.
        match self.verbosity {
            Verbosity::Concise => {}
            Verbosity::Full => self.print_message(INFO_SYMBOL, FRAME, message),
        }
    }
    fn step(&self, current: usize, total: usize, message: &str) {
        let index = paint(FRAME, &format!("[{current}/{total}]"), self.rendering);
        self.print_message("", Style::new(), &format!("{index} {message}"));
    }
    fn raw_log(&self, message: &str) {
        self.emit(message);
    }
}

impl DiagnosticSink for TerminalReporter {
    fn report(&self, diagnostic: &Diagnostic) {
        self.ensure_spacer();
        for line in diagnostic_lines(diagnostic, self.verbosity, self.rendering) {
            self.emit(&line);
        }
    }
}

impl RunSummary for TerminalReporter {
    fn summary(&self, snapshot: &TelemetrySnapshot, reasons: &[Reason]) {
        // Leave the finished stage bars on screen and append the report below them. indicatif's
        // `println` writes above the managed bars (shoving them) and its throttled redraw clips a
        // burst of appended bars on a fast exit, so the report is written directly instead. The bars'
        // steady-tick threads are stopped first so no late redraw paints over the appended text.
        self.stages.disable_steady_ticks();
        if let Some(bar) = self.progress_bar.lock().as_ref() {
            bar.disable_steady_tick();
        }
        // The last bar line is not newline-terminated, so a leading newline breaks off that line
        // before the report's own leading blank separates it from the bars.
        let report = run_summary::render(snapshot, reasons, self.rendering).join("\n");
        eprintln!("\n{report}");
    }
}

impl ProgressReporter for TerminalReporter {
    fn start_progress(&self, message: &str) {
        let mut progress = self.progress_bar.lock();
        if progress.is_some() {
            return;
        }

        let bar = self.multi_progress.add(ProgressBar::new(0));
        bar.set_style(style::progress());
        bar.set_prefix(message.to_owned());
        bar.enable_steady_tick(TICK_INTERVAL);
        *progress = Some(bar);
    }

    fn update_progress(&self, message: &str) {
        if let Some(bar) = self.progress_bar.lock().as_ref() {
            bar.set_message(message.to_owned());
        }
    }

    fn progress_set_length(&self, length: u64) {
        if let Some(bar) = self.progress_bar.lock().as_ref() {
            bar.set_length(length);
        }
    }

    fn progress_inc(&self) {
        if let Some(bar) = self.progress_bar.lock().as_ref() {
            bar.inc(1);
        }
    }

    fn stop_progress(&self) {
        if let Some(bar) = self.progress_bar.lock().take() {
            bar.finish_and_clear();
        }
    }
}

impl StageReporter for TerminalReporter {
    fn set_quiet_stages(&self, output: StageOutput) {
        self.stages.set_quiet(output);
    }

    fn enter_stage(&self, stage: Box<dyn ProgressStage>, execution_time: ExecutionTime) -> StageGuard {
        StageGuard::new(self.stages.enter(stage.id(), stage.label(), execution_time))
    }

    fn pre_register_stages(&self, stages: &[Box<dyn ProgressStage>]) {
        self.stages.pre_register(stages);
    }

    fn stage_set_message(&self, id: StageId, message: &str) {
        self.stages.set_message(id, message);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::terminal::reporter::TerminalReporter,
        reporter::{DiagnosticSink, MessageLog, ProgressStage, StageOutput, StageReporter},
        stage_id::{StageId, StageLabel},
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, run_code::RunCode},
        diagnostic_builder::DiagnosticBuilder,
        severity::Severity,
        verbosity::Verbosity,
    };
    use cassiopeia_terminal_style::rendering::Rendering;
    use execution_time::ExecutionTime;

    /// A stage identity for the registry tests.
    #[derive(Debug)]
    struct NamedStage(&'static str);

    impl ProgressStage for NamedStage {
        fn id(&self) -> StageId {
            StageId::new(self.0)
        }
        fn label(&self) -> StageLabel {
            StageLabel::new(self.0)
        }
    }

    #[test]
    fn quiet_mode_suppresses_stage_creation() {
        let reporter = TerminalReporter::new(Verbosity::Concise);
        reporter.set_quiet_stages(StageOutput::Quiet);
        let guard = reporter.enter_stage(Box::new(NamedStage("s")), ExecutionTime::start());
        guard.inc_by(5);
        drop(guard);

        assert_eq!(reporter.stages.len(), 0);
    }

    #[test]
    fn a_visible_stage_is_registered_and_can_be_finished() {
        let reporter = TerminalReporter::new(Verbosity::Concise);
        let guard = reporter.enter_stage(Box::new(NamedStage("s")), ExecutionTime::start());
        assert_eq!(reporter.stages.len(), 1);

        guard.inc_by(3);
        drop(guard);

        assert!(!reporter.stages.is_running(StageId::new("s")));
    }

    #[test]
    fn logging_helpers_do_not_panic() {
        let reporter = TerminalReporter::new(Verbosity::Full);
        reporter.info("info");
        reporter.success("success");
        reporter.debug("debug");
        reporter.step(1, 3, "step");
        reporter.raw_log("raw");
        reporter.report(
            &DiagnosticBuilder::new(Severity::Error, DiagnosticCode::Run(RunCode::Failed), "boom")
                .with_cause("root")
                .build(),
        );
    }

    // Under the project's redirect-then-read test convention stderr is captured, so indicatif hides
    // its bars and the reporter takes the plain, straight-to-stderr path.
    #[test]
    fn a_captured_stderr_renders_plain_text() {
        assert_eq!(TerminalReporter::new(Verbosity::Concise).rendering, Rendering::Plain);
    }

    #[test]
    fn concurrent_workers_share_a_stage_lifecycle_until_the_last_finishes() {
        let reporter = TerminalReporter::new(Verbosity::Concise);
        let id = StageId::new("resolver");
        let first = reporter.enter_stage(Box::new(NamedStage("resolver")), ExecutionTime::start());
        let second = reporter.enter_stage(Box::new(NamedStage("resolver")), ExecutionTime::start());

        drop(first);
        assert!(reporter.stages.is_running(id));

        drop(second);
        assert!(!reporter.stages.is_running(id));
    }

    #[test]
    fn a_live_annotation_reaches_the_registered_stage() {
        let reporter = TerminalReporter::new(Verbosity::Concise);
        let guard = reporter.enter_stage(Box::new(NamedStage("writer")), ExecutionTime::start());

        reporter.stage_set_message(StageId::new("writer"), "~12ms");

        drop(guard);
    }
}

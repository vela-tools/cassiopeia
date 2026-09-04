//! Tracing backend for the reporter.

use crate::{
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageReporter},
    stage_handle::StageHandle,
    stage_id::StageId,
};
use cassiopeia_common::telemetry::{run::TelemetrySnapshot, stage_metrics::RateReader};
use cassiopeia_diagnostic::{diagnostic::Diagnostic, reason::Reason, severity::Severity};
use execution_time::ExecutionTime;

/// Reporter that delegates all messages to the tracing framework.
#[derive(Debug, Default)]
pub struct TracingReporter;

impl TracingReporter {
    /// Builds a tracing-backed reporter.
    #[must_use]
    pub const fn new() -> TracingReporter {
        TracingReporter
    }
}

impl MessageLog for TracingReporter {
    fn info(&self, message: &str) {
        tracing::info!("{}", message);
    }
    fn success(&self, message: &str) {
        tracing::info!(status = "success", "{}", message);
    }
    fn debug(&self, message: &str) {
        tracing::debug!("{}", message);
    }
    fn step(&self, current: usize, total: usize, message: &str) {
        tracing::info!(step = current, total = total, "{}", message);
    }
    fn raw_log(&self, message: &str) {
        tracing::info!("{}", message);
    }
}

impl DiagnosticSink for TracingReporter {
    /// Emits the diagnostic at its own level with every field attached.
    ///
    /// Verbosity is a terminal concern and is deliberately ignored here: a structured log is read by
    /// a query, not by eye, so it always carries the whole diagnostic.
    fn report(&self, diagnostic: &Diagnostic) {
        let code = diagnostic.code().to_string();
        let causes = diagnostic.causes().len();
        let occurrences = diagnostic.occurrences().get();
        match diagnostic.severity() {
            Severity::Error => tracing::error!(
                code = code,
                occurrences = occurrences,
                causes = causes,
                explanation = diagnostic.explanation(),
                "{}",
                diagnostic.headline()
            ),
            Severity::Warning => tracing::warn!(
                code = code,
                occurrences = occurrences,
                causes = causes,
                explanation = diagnostic.explanation(),
                "{}",
                diagnostic.headline()
            ),
        }
    }
}

impl RunSummary for TracingReporter {
    fn summary(&self, snapshot: &TelemetrySnapshot, reasons: &[Reason]) {
        let counters = snapshot.counters;
        tracing::info!(
            event = "pipeline_summary",
            elapsed_ms = snapshot.elapsed.as_millis(),
            input_records = counters.input_records,
            fragments_created = counters.fragments_created,
            unique_entities = counters.unique_entities,
            entities_written = counters.entities_written,
            errors = counters.errors,
            warnings = counters.warnings,
            bytes_read = counters.bytes_read,
            bytes_written = counters.bytes_written,
            stages = snapshot.stages.len(),
            channels = snapshot.channels.len(),
            reasons = reasons.len(),
            "Pipeline run telemetry"
        );
        for reason in reasons {
            tracing::info!(
                event = "pipeline_reason",
                code = reason.code().to_string(),
                severity = reason.severity().to_string(),
                count = reason.count(),
                example = reason.example(),
                "Run failure reason"
            );
        }
    }
}

impl ProgressReporter for TracingReporter {
    fn start_progress(&self, message: &str) {
        tracing::info!(event = "progress_start", "{}", message);
    }
    fn update_progress(&self, message: &str) {
        tracing::debug!(event = "progress_update", "{}", message);
    }
    fn progress_set_length(&self, length: u64) {
        tracing::debug!(event = "progress_set_length", length = length);
    }
    fn progress_inc(&self) {
        // A per-item increment is too verbose for a structured log.
    }
    fn stop_progress(&self) {
        tracing::info!(event = "progress_stop");
    }
}

impl StageReporter for TracingReporter {
    fn enter_stage(&self, stage: Box<dyn ProgressStage>, _execution_time: ExecutionTime) -> StageGuard {
        let id = stage.id();
        tracing::info!(stage_id = id.as_ref(), stage_label = stage.label().as_ref(), "Stage started");
        StageGuard::new(Box::new(TracingStageHandle { id }))
    }

    fn pre_register_stages(&self, _stages: &[Box<dyn ProgressStage>]) {
        // Stage ordering is a terminal-display concern; nothing to pre-register for tracing.
    }
}

/// Logs one stage's lifecycle events, dropping the per-batch counting a structured log cannot use.
struct TracingStageHandle {
    id: StageId,
}

impl StageHandle for TracingStageHandle {
    fn inc_by(&self, _count: u64) {
        // Progress counts are too verbose for a structured log; the run summary reports the totals.
    }

    fn set_length(&self, length: u64) {
        tracing::debug!(stage_id = self.id.as_ref(), length = length, "Stage length set");
    }

    fn warn(&self) {
        tracing::warn!(stage_id = self.id.as_ref(), "Stage warning reported");
    }

    fn warn_inc_by(&self, count: u64) {
        tracing::warn!(stage_id = self.id.as_ref(), count = count, "Stage warnings reported");
    }

    fn attach_rate(&self, _reader: RateReader) {
        // A structured log renders no live rate.
    }

    fn finish(&self) {
        tracing::info!(stage_id = self.id.as_ref(), "Stage finished");
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::tracing::TracingReporter,
        reporter::{MessageLog, ProgressStage, StageOutput, StageReporter},
        stage_id::{StageId, StageLabel},
    };
    use execution_time::ExecutionTime;

    /// A stage identity for the logging test; nothing asserts on the labels.
    #[derive(Debug)]
    struct TestStage;

    impl ProgressStage for TestStage {
        fn id(&self) -> StageId {
            StageId::new("s")
        }
        fn label(&self) -> StageLabel {
            StageLabel::new("S")
        }
    }

    #[test]
    fn logging_and_stage_calls_do_not_panic_without_a_subscriber() {
        let reporter = TracingReporter::new();

        reporter.info("info");
        reporter.debug("debug");
        reporter.set_quiet_stages(StageOutput::Quiet);
        let guard = reporter.enter_stage(Box::new(TestStage), ExecutionTime::start());
        guard.set_length(10);
        guard.inc_by(10);
        guard.warn_inc_by(2);
        drop(guard);
    }
}

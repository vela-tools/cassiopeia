//! Core trait definitions for the reporter library.
//!
//! The reporter contract is split by capability so a consumer depends on only the narrow slice it
//! uses: [`MessageLog`] for one-off log lines, [`DiagnosticSink`] for a structured failure,
//! [`RunSummary`] for the end-of-run report, [`ProgressReporter`] for a single progress indicator,
//! and [`StageReporter`] for the managed multi-stage display. [`Reporter`] is the convenience
//! supertrait for holders (such as the global reporter) that need all five.

use crate::{
    guard::StageGuard,
    stage_id::{StageId, StageLabel},
};
use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{diagnostic::Diagnostic, reason::Reason};
use execution_time::ExecutionTime;
use std::fmt::Debug;

/// Whether a stage's visual progress is drawn or suppressed.
///
/// In scheduled runs the multi-stage progress bars are replaced by compact log lines, so stages are
/// tracked internally but produce no terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutput {
    /// Track stages internally but draw nothing.
    Quiet,
    /// Draw the stage progress bars.
    Visible,
}

/// A progress stage: something with a stable identity and a display name.
///
/// Applications implement this for their own stage enums so the reporter can key on the identity
/// while showing the label.
pub trait ProgressStage: Send + Sync + Debug + 'static {
    /// The stage's stable identity, used for internal mapping.
    fn id(&self) -> StageId;
    /// The stage's human-readable label, used for display.
    fn label(&self) -> StageLabel;
}

/// Emitting one-off narrative log lines, indexed progress, and raw-line output.
///
/// This capability carries no severity above informational: anything that went wrong is a
/// [`Diagnostic`] and goes to a [`DiagnosticSink`], so it can be tiered, grouped, and tallied rather
/// than stringified at the call site.
pub trait MessageLog: Send + Sync {
    /// Reports an informational message.
    fn info(&self, message: &str);
    /// Reports a success.
    fn success(&self, message: &str);
    /// Reports a debug-level message, surfaced only at the verbose level.
    fn debug(&self, message: &str);
    /// Reports indexed progress in a process (e.g. "1/5: Loading data").
    fn step(&self, current: usize, total: usize, message: &str);
    /// Reports a raw message without symbols or prefixes (useful for summaries).
    fn raw_log(&self, message: &str);
}

/// Receiving a structured failure.
///
/// A consumer that only ever reports something going wrong depends on this and nothing else, which
/// is strictly narrower than the whole reporter: the `@context` resolver and the broker's context
/// mode both take one of these rather than a log.
pub trait DiagnosticSink: Send + Sync {
    /// Reports one diagnostic.
    fn report(&self, diagnostic: &Diagnostic);
}

/// Emitting the end-of-run report.
pub trait RunSummary: Send + Sync {
    /// Emits one summary for a completed pipeline run, together with the reasons its failures
    /// grouped under.
    fn summary(&self, snapshot: &TelemetrySnapshot, reasons: &[Reason]);
}

/// Driving a single, one-off progress indicator.
pub trait ProgressReporter: Send + Sync {
    /// Starts a one-off progress indicator.
    fn start_progress(&self, message: &str);
    /// Updates the current one-off progress message.
    fn update_progress(&self, message: &str);
    /// Sets the total length for the one-off progress.
    fn progress_set_length(&self, length: u64);
    /// Increments the one-off progress by one.
    fn progress_inc(&self);
    /// Stops the one-off progress indicator.
    fn stop_progress(&self);
}

/// Driving the managed multi-stage progress display.
///
/// The contract is deliberately narrow: a reporter registers stages and hands out a
/// [`StageGuard`] for each entered one. Everything a *running* stage does (counting, warning,
/// declaring a length, finishing) goes through the [`StageHandle`](crate::stage_handle::StageHandle)
/// that guard owns, not back through the reporter, so a stage's hot path never re-resolves itself
/// through the backend's shared id-keyed registry.
pub trait StageReporter: Send + Sync {
    /// Sets whether stage progress is drawn or suppressed.
    ///
    /// When [`StageOutput::Quiet`], the stage methods still track internally but produce no terminal
    /// output.
    fn set_quiet_stages(&self, _output: StageOutput) {}

    /// Enters a new progress stage, returning an RAII guard that finishes the stage when dropped.
    ///
    /// The guard carries the backend's handle for the stage, so entering is the only id lookup a
    /// stage performs for its whole lifetime.
    fn enter_stage(&self, stage: Box<dyn ProgressStage>, execution_time: ExecutionTime) -> StageGuard;

    /// Pre-registers stages to establish their order in the display.
    fn pre_register_stages(&self, stages: &[Box<dyn ProgressStage>]);

    /// Publishes a live free-form annotation for a running stage (e.g. a metric readout).
    ///
    /// Keyed by id rather than routed through a guard because the publisher is not the stage worker:
    /// a broker writer's delivery pool annotates the writer stage from its own threads, holding no
    /// guard of its own. It is called once per flush, never per item.
    fn stage_set_message(&self, _id: StageId, _message: &str) {}
}

/// The full reporter contract: message logging, diagnostics, the run summary, one-off progress, and
/// managed stages.
///
/// Holders that need every capability (such as the global reporter) depend on this supertrait;
/// consumers that need one capability depend on the narrowest trait instead.
pub trait Reporter: MessageLog + DiagnosticSink + RunSummary + ProgressReporter + StageReporter {}

impl<T> Reporter for T where T: MessageLog + DiagnosticSink + RunSummary + ProgressReporter + StageReporter {}

#[cfg(test)]
mod tests {
    use crate::{
        backend::noop::NoopReporter,
        reporter::{Reporter, StageOutput},
    };

    #[test]
    fn a_type_implementing_every_capability_is_a_reporter() {
        // Compiles only because the blanket impl makes `NoopReporter` a full `Reporter`.
        let reporter: &dyn Reporter = &NoopReporter::new();
        reporter.info("ok");
    }

    #[test]
    fn stage_output_variants_are_distinct() {
        assert_ne!(StageOutput::Quiet, StageOutput::Visible);
    }
}

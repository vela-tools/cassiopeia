use crate::pipeline_stage::PipelineStage;
use cassiopeia_common::telemetry::run::TelemetrySnapshot;

/// A progress snapshot the pipeline emits as a run advances.
#[derive(Debug, Clone)]
pub struct PipelineProgress {
    /// The stage the snapshot describes.
    pub stage: PipelineStage,
    /// How many items the stage has processed so far.
    pub processed: u64,
    /// The total the stage expects to process, when known.
    pub total: Option<u64>,
    /// Structured telemetry captured with this progress event, when available.
    pub telemetry: Option<TelemetrySnapshot>,
}

/// The outcome of one completed run cycle.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Records read from the sources.
    pub input_records: u64,
    /// Entities written to the destination.
    pub output_entities: u64,
    /// Errors observed during the run.
    pub errors: u64,
    /// Warnings observed during the run.
    pub warnings: u64,
    /// Complete run telemetry, including stage and channel snapshots.
    pub telemetry: TelemetrySnapshot,
}

/// Receives the pipeline's high-level lifecycle events.
///
/// This is the extension point a caller (a CLI summary, a TUI) uses to observe a run beyond the
/// terminal reporter. Every method has a default no-op, so an observer overrides only what it needs.
pub trait RunObserver: Send + Sync {
    /// Called as a stage reports progress.
    fn on_progress(&self, _progress: &PipelineProgress) {}
    /// Called once a run cycle completes, with its tallied result.
    fn on_complete(&self, _result: &PipelineResult) {}
}

/// An observer that discards every event, the default when a caller does not supply one.
pub struct NoopObserver;

impl RunObserver for NoopObserver {}

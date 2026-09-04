use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        stage_env::StageEnv,
        stream_outcome::StreamOutcome,
    },
};
use cassiopeia_common::{
    batch::Batch,
    channel::ChannelReceiver,
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, component::StageComponent, run::RunTelemetry},
};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::guard::StageGuard;
use cassiopeia_writer::{
    run_outcome::RunOutcome,
    writer::{Writer, WriterProgress},
};
use std::{ops::ControlFlow, sync::Arc};

/// Writes each batch of entities to the destination and reports a write failure downstream; once the
/// stream ends it finalizes the writer, flushing any buffered output.
struct WriterProcessor {
    /// The destination writer, the pipeline sink.
    writer: Box<dyn Writer>,
    telemetry: Arc<RunTelemetry>,
    reported: WriterProgress,
}

impl PumpProcessor for WriterProcessor {
    type In = NgsiLdEntity;
    type Out = ();

    fn process(&mut self, batch: Batch<NgsiLdEntity>, stage: &StageGuard, tx: &BatchSender<()>) -> ControlFlow<()> {
        let service = stage.service_span();
        let result = self.writer.write_batch(Vec::from(batch));
        drop(service);

        self.report_progress(stage);
        if let Err(error) = result {
            stage.fail(1);
            self.telemetry.add_errors(1);
            self.reported.failed = self.reported.failed.saturating_add(1);
            let _ = tx.send(Signal::Error(PipelineError::from(error)));
        }
        ControlFlow::Continue(())
    }

    fn finalize(&mut self, outcome: StreamOutcome, stage: &StageGuard, tx: &BatchSender<()>) {
        // A clean stream commits the writer's output; a failed or cancelled one aborts it, so a
        // staging writer never leaves a half-populated destination behind.
        let run_outcome = match outcome {
            StreamOutcome::Clean => RunOutcome::Committed,
            StreamOutcome::Failed => RunOutcome::Aborted,
        };
        let service = stage.service_span();
        let result = self.writer.finalize(run_outcome);
        drop(service);
        match result {
            Ok(stats) => {
                let written = u64::try_from(stats.written).unwrap_or(u64::MAX);
                let failed = u64::try_from(stats.failed).unwrap_or(u64::MAX);
                self.report_totals(stage, written, failed, stats.bytes_written);
                stage.component_time(StageComponent::Request, stats.request_time);
            }
            Err(error) => {
                stage.fail(1);
                self.telemetry.add_errors(1);
                let _ = tx.send(Signal::Error(PipelineError::from(error)));
            }
        }
    }
}

impl WriterProcessor {
    fn report_progress(&mut self, stage: &StageGuard) {
        let progress = self.writer.progress();
        self.report_totals(
            stage,
            u64::try_from(progress.written).unwrap_or(u64::MAX),
            u64::try_from(progress.failed).unwrap_or(u64::MAX),
            progress.bytes_written,
        );
    }

    /// Advances the stage by whatever the writer has confirmed since the last report.
    ///
    /// The writer reports cumulative totals, so the deltas are what this stage has not yet counted;
    /// they go onto the bar and into the telemetry in one call each, however large the delta is.
    fn report_totals(&mut self, stage: &StageGuard, written: u64, failed: u64, bytes_written: u64) {
        let written_delta = written.saturating_sub(u64::try_from(self.reported.written).unwrap_or(u64::MAX));
        let failed_delta = failed.saturating_sub(u64::try_from(self.reported.failed).unwrap_or(u64::MAX));
        let bytes_delta = bytes_written.saturating_sub(self.reported.bytes_written);
        if written_delta > 0 {
            stage.inc_by(written_delta);
        }
        if failed_delta > 0 {
            stage.fail(failed_delta);
        }
        self.telemetry.add_entities_written(written_delta);
        self.telemetry.add_errors(failed_delta);
        self.telemetry.add_bytes_written(bytes_delta);
        self.reported = WriterProgress {
            written: usize::try_from(written).unwrap_or(usize::MAX),
            failed: usize::try_from(failed).unwrap_or(usize::MAX),
            bytes_written,
        };
    }
}

/// Spawns the writer, the pipeline sink. It writes each batch of entities to the destination and
/// reports a write failure downstream; once the stream ends it finalizes the writer.
pub(crate) fn spawn_writer_thread(
    receiver: ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>>,
    writer: Box<dyn Writer>,
    resolved_count: u64,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<()>, PipelineError>> {
    run_pump_stage(
        WriterProcessor {
            writer,
            telemetry: Arc::clone(&env.telemetry),
            reported: WriterProgress::default(),
        },
        receiver,
        PumpConfig {
            stage: PipelineStage::Writer,
            set_length: Some(resolved_count),
            expected_stops: 1,
            boundary: ChannelBoundary::to_sink(Stage::Writer),
            env,
        },
    )
}

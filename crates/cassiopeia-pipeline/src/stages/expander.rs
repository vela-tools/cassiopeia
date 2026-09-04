use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{skipped_records::SkippedRecords, stage_env::StageEnv},
};
use cassiopeia_common::{
    batch::Batch,
    channel::{ChannelReceiver, ChannelSender},
    signal::Signal,
    stage::Stage,
    telemetry::run::RunTelemetry,
};
use cassiopeia_diagnostic::severity::Severity;
use cassiopeia_expander::{error::ExpanderError, expander::Expander};
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, record::Record};
use cassiopeia_reporter::guard::StageGuard;
use execution_time::ExecutionTime;
use std::{mem, ops::ControlFlow, thread::spawn};

/// The fragment channel produced by the expander stage.
type FragmentSender = ChannelSender<Signal<Batch<Mapped<Fragment>>, PipelineError>>;

/// Accumulates expanded fragments and hands them onto the shared resolver intake a batch at a time.
///
/// One record can expand into several fragments, so the batch the expander emits is not the batch it
/// received; this buffers across records until the configured size is reached.
struct FragmentEmitter<'a> {
    tx: &'a FragmentSender,
    batch_size: usize,
    pending: Batch<Mapped<Fragment>>,
}

impl<'a> FragmentEmitter<'a> {
    /// Creates an emitter that flushes every `batch_size` fragments.
    fn new(tx: &'a FragmentSender, batch_size: usize) -> FragmentEmitter<'a> {
        FragmentEmitter {
            tx,
            batch_size,
            pending: Batch::with_capacity(batch_size),
        }
    }

    /// Accepts one record's fragments, flushing whenever the batch fills.
    fn accept(&mut self, fragments: Vec<Mapped<Fragment>>, stage: &StageGuard, run: &RunTelemetry) -> ControlFlow<()> {
        for fragment in fragments {
            self.pending.push(fragment);
            if self.pending.len() >= self.batch_size {
                self.flush(stage, run)?;
            }
        }
        ControlFlow::Continue(())
    }

    /// Hands whatever has accumulated downstream, counting it once.
    fn flush(&mut self, stage: &StageGuard, run: &RunTelemetry) -> ControlFlow<()> {
        if self.pending.is_empty() {
            return ControlFlow::Continue(());
        }
        let batch = mem::replace(&mut self.pending, Batch::with_capacity(self.batch_size));
        let count = batch.count();
        if stage.measure_output_wait(|| self.tx.send(Signal::Data(batch)).is_ok()) {
            stage.inc_by(count);
            run.add_fragments_created(count);
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}

/// Spawns the expander for one lane. It owns the boxed expander outright (a single thread drives it,
/// so no lock is needed) and counts both ingested records and emitted fragments as it runs.
///
/// Fragments are written straight onto the shared resolver intake channel (`tx`, one clone per lane),
/// so every lane merges into the resolver with no intermediate per-lane channel or bridge thread.
///
/// A record that fails to expand is registered as a stage warning and skipped, so one bad record
/// does not abort the lane. The refusals are grouped by reason and reported once each with a count,
/// rather than surfacing as an anonymous tally. Cancellation, upstream termination, or a lost
/// downstream channel stops processing.
pub(crate) fn spawn_expander_thread(
    expander: Box<dyn Expander>,
    receiver: ChannelReceiver<Signal<Batch<Record>, PipelineError>>,
    tx: FragmentSender,
    batch_size: usize,
    env: &StageEnv,
) {
    let reporter = env.reporter;
    let controller = env.controller.clone();
    let telemetry = env.telemetry.clone();

    spawn(move || {
        let ingestor_time = ExecutionTime::start();
        let expander_time = ExecutionTime::start();
        let ingestor_stage = reporter
            .enter_stage(Box::new(PipelineStage::Ingestor), ingestor_time)
            .with_telemetry(telemetry.start_stage(Stage::Ingestor));
        let expander_stage = reporter
            .enter_stage(Box::new(PipelineStage::Expander), expander_time)
            .with_telemetry(telemetry.start_stage(Stage::Expander));

        if tx.send(Signal::Start).is_err() {
            return;
        }

        let mut emitter = FragmentEmitter::new(&tx, batch_size);

        // The wait belongs to the expander: it is the consumer of the ingestor's records. The
        // ingestor stage's own input wait is measured where it waits for a payload to ingest.
        while let Ok(signal) = expander_stage.measure_input_wait(|| receiver.recv()) {
            if controller.should_cancel() {
                break;
            }
            match signal {
                Signal::Data(records) => {
                    let count = records.count();
                    ingestor_stage.received(count);
                    ingestor_stage.inc_by(count);
                    expander_stage.received(count);

                    // The service span covers expansion only; the handoff onto the resolver intake
                    // is output wait, measured inside the emitter.
                    let service = expander_stage.service_span();
                    let results = expander.expand_batch(Vec::from(records));
                    drop(service);

                    let mut skipped = SkippedRecords::<_, ExpanderError>::new();
                    let mut broken = false;
                    for result in results {
                        match result {
                            Ok(fragments) => {
                                if emitter.accept(fragments, &expander_stage, &telemetry).is_break() {
                                    broken = true;
                                    break;
                                }
                            }
                            Err(error) => skipped.record(error),
                        }
                    }
                    if !skipped.is_empty() {
                        let failed = skipped.total();
                        expander_stage.warn_inc_by(failed);
                        telemetry.add_warnings(failed);
                        skipped.report(reporter, Severity::Warning);
                    }
                    if broken {
                        return;
                    }
                }
                Signal::Stop => break,
                Signal::Error(error) => {
                    if tx.send(Signal::Error(error)).is_err() {
                        return;
                    }
                }
                Signal::Meta(meta) => {
                    if tx.send(Signal::Meta(meta)).is_err() {
                        return;
                    }
                }
                Signal::Start => {}
            }
        }

        // The lane ends mid-batch whenever its fragment count is not a multiple of the batch size.
        let _ = emitter.flush(&expander_stage, &telemetry);

        let _ = tx.send(Signal::Stop);
    });
}

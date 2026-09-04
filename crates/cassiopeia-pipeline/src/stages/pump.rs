use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{stage_env::StageEnv, stream_outcome::StreamOutcome},
};
use cassiopeia_common::{
    batch::Batch,
    channel::{ChannelReceiver, ChannelSender},
    signal::Signal,
    telemetry::channel_boundary::ChannelBoundary,
};
use cassiopeia_reporter::guard::StageGuard;
use execution_time::ExecutionTime;
use std::{ops::ControlFlow, thread::spawn};

/// The output channel a pump stage writes batches of its emitted item onto.
pub(crate) type BatchSender<T> = ChannelSender<Signal<Batch<T>, PipelineError>>;

/// The per-batch work a signal-pump stage performs.
///
/// The runner ([`run_pump_stage`]) owns the channel plumbing: the `Start`/`Stop` bracketing, the
/// `Error`/`Meta` forwarding, and cancellation polling, while an implementor owns every counting
/// and forwarding decision. The runner counts nothing itself, so a stage's progress tallies live
/// entirely in [`process`](PumpProcessor::process) and [`finalize`](PumpProcessor::finalize).
///
/// Work arrives already grouped: producers batch, so the runner hands each received [`Batch`]
/// straight to the processor without buffering. Single-item processing is a batch of one, not a
/// separate code path.
///
/// `&mut self` because a stage may own mutable state the runner must not split across borrows, such
/// as a writer's `write`/`finalize` pair, or a validator's report collector.
///
/// The `stage` guard is the single handle a processor records through: its counting and timing
/// methods update both the progress display and the run telemetry, so a stage never records the same
/// event twice. Its counting methods take counts, so a processor reports a whole batch in one call.
pub(crate) trait PumpProcessor: Send + 'static {
    /// The item type the stage consumes from each `Data` signal's batch.
    type In: Send + 'static;

    /// The item type the stage emits downstream.
    type Out: Send + 'static;

    /// Processes one batch; returns [`ControlFlow::Break`] to wind the stage down (its downstream
    /// receiver is gone, or it hit a fatal condition it has already reported).
    fn process(&mut self, batch: Batch<Self::In>, stage: &StageGuard, tx: &BatchSender<Self::Out>) -> ControlFlow<()>;

    /// Runs once after the input stream ends, before the closing `Stop` is sent. `outcome` reports
    /// whether the stream ended cleanly or after a failure. The default does nothing; a stage
    /// overrides it to flush a writer or emit a report.
    fn finalize(&mut self, _outcome: StreamOutcome, _stage: &StageGuard, _tx: &BatchSender<Self::Out>) {}
}

/// The settings a pump stage runs under, independent of its per-batch processor.
pub(crate) struct PumpConfig {
    /// The stage this pump reports progress as.
    pub(crate) stage: PipelineStage,

    /// The progress-bar length to declare up front, when the count is known ahead of time.
    pub(crate) set_length: Option<u64>,

    /// How many upstream `Stop` signals must arrive before the stage finishes: one per input
    /// channel merged into this stage.
    pub(crate) expected_stops: usize,

    /// The boundary this stage's output queue sits on, naming its real downstream consumer so the run
    /// summary labels the channel correctly (the validator feeds the fold or the writer depending on
    /// the run).
    pub(crate) boundary: ChannelBoundary,

    /// The shared execution environment: channel capacity, reporter, and cancellation controller.
    pub(crate) env: StageEnv,
}

/// Spawns a signal-pump stage on its own thread and returns the configured channel the next stage reads.
///
/// The stage enters its progress guard, optionally declares its length, and brackets its output with
/// `Start` and `Stop`. Between them it pumps the input: each `Data` batch goes to the processor,
/// `Error` and `Meta` are forwarded verbatim, and `Stop` counts down `expected_stops`. Cancellation
/// is polled once per signal, so the stage winds down between batches.
pub(crate) fn run_pump_stage<P: PumpProcessor>(
    mut processor: P,
    receiver: ChannelReceiver<Signal<Batch<P::In>, PipelineError>>,
    config: PumpConfig,
) -> ChannelReceiver<Signal<Batch<P::Out>, PipelineError>> {
    let PumpConfig {
        stage,
        set_length,
        expected_stops,
        boundary,
        env,
    } = config;
    let reporter = env.reporter;
    let controller = env.controller.clone();
    let telemetry = env.telemetry.clone();
    let (tx, rx) = env.channel(Some(boundary));

    spawn(move || {
        let execution_time = ExecutionTime::start();
        // Every pump stage measures a telemetry stage; only the run-bracketing scheduler maps to
        // `None`, and it never runs through the pump.
        let guard = match stage.stage() {
            Some(measured) => reporter
                .enter_stage(Box::new(stage), execution_time)
                .with_telemetry(telemetry.start_stage(measured)),
            None => reporter.enter_stage(Box::new(stage), execution_time),
        };
        if let Some(length) = set_length {
            guard.set_length(length);
        }
        let _ = tx.send(Signal::Start);

        let mut stops_remaining = expected_stops;
        // Tracks whether the stream forwarded an error, so a clean end is distinguishable from a
        // failed one at finalize; combined with cancellation below.
        let mut errored = false;

        while let Ok(signal) = guard.measure_input_wait(|| receiver.recv()) {
            if controller.should_cancel() {
                break;
            }
            match signal {
                Signal::Data(batch) => {
                    guard.received(batch.count());
                    if processor.process(batch, &guard, &tx).is_break() {
                        errored = true;
                        break;
                    }
                }
                Signal::Stop => {
                    stops_remaining = stops_remaining.saturating_sub(1);
                    if stops_remaining == 0 {
                        break;
                    }
                }
                Signal::Error(error) => {
                    errored = true;
                    let _ = tx.send(Signal::Error(error));
                }
                Signal::Meta(meta) => {
                    let _ = tx.send(Signal::Meta(meta));
                }
                Signal::Start => {}
            }
        }

        // A forwarded error, or a cancellation that broke the loop, marks the stream as failed.
        let outcome = if errored || controller.should_cancel() {
            StreamOutcome::Failed
        } else {
            StreamOutcome::Clean
        };
        processor.finalize(outcome, &guard, &tx);

        let _ = tx.send(Signal::Stop);
    });

    rx
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::RunController,
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
        channel::{ChannelPolicy, channel},
        signal::{MetaSignal, Signal},
        stage::Stage,
        telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
    };
    use cassiopeia_reporter::{backend::noop::NoopReporter, guard::StageGuard, reporter::Reporter};
    use std::{
        num::NonZeroUsize,
        ops::ControlFlow,
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    /// The process-lifetime reporter every pump test drives; its guards count into nothing, which is
    /// correct here since the tests assert channel behaviour, not progress totals.
    static NOOP: NoopReporter = NoopReporter::new();

    /// A shared recorder for the batches a processor saw, as plain vectors for easy assertions.
    type Batches = Arc<Mutex<Vec<Vec<u32>>>>;
    /// A stream of signals fed to, or drained from, a pump.
    type Signals = Vec<Signal<Batch<u32>, PipelineError>>;
    /// A shared slot for the stream outcome a processor's finalize was handed.
    type CapturedOutcome = Arc<Mutex<Option<StreamOutcome>>>;
    /// What [`run`] hands back: the batch recorder, the captured finalize outcome, and the output.
    type RunResult = (Batches, CapturedOutcome, Signals);

    /// A controller whose cancellation flag a test can flip.
    struct ToggleController {
        cancel: Arc<AtomicBool>,
    }

    impl RunController for ToggleController {
        fn should_cancel(&self) -> bool {
            self.cancel.load(Ordering::SeqCst)
        }
    }

    /// A processor that records every batch it sees and forwards it downstream unchanged, so a test
    /// can read back exactly what the runner handed it.
    struct RecordingProcessor {
        batches: Batches,
        outcome: CapturedOutcome,
    }

    impl PumpProcessor for RecordingProcessor {
        type In = u32;
        type Out = u32;

        fn process(&mut self, batch: Batch<u32>, _stage: &StageGuard, tx: &BatchSender<u32>) -> ControlFlow<()> {
            self.batches.lock().unwrap().push(batch.iter().copied().collect());
            if tx.send(Signal::Data(batch)).is_err() {
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        }

        fn finalize(&mut self, outcome: StreamOutcome, _stage: &StageGuard, _tx: &BatchSender<u32>) {
            *self.outcome.lock().unwrap() = Some(outcome);
        }
    }

    /// Builds a stage environment over the no-op reporter and a controller whose flag the test holds.
    fn env(cancel: Arc<AtomicBool>) -> StageEnv {
        StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter: &NOOP as &'static dyn Reporter,
            controller: Arc::new(ToggleController { cancel }),
            telemetry: Arc::new(RunTelemetry::new()),
        }
    }

    /// Wraps a list of values as one data signal carrying a batch.
    fn data(values: &[u32]) -> Signal<Batch<u32>, PipelineError> {
        Signal::Data(Batch::from(values.to_vec()))
    }

    /// Feeds a prepared list of signals into a fresh pump and returns the recorder handles and the
    /// full list of output signals, drained to completion.
    fn run(expected_stops: usize, cancel: Arc<AtomicBool>, inputs: Signals) -> RunResult {
        let batches = Arc::new(Mutex::new(Vec::new()));
        let outcome = Arc::new(Mutex::new(None));
        let processor = RecordingProcessor {
            batches: Arc::clone(&batches),
            outcome: Arc::clone(&outcome),
        };

        let (in_tx, in_rx) = channel(ChannelPolicy::Bounded(NonZeroUsize::new(1024).unwrap()));
        for signal in inputs {
            in_tx.send(signal).unwrap();
        }
        drop(in_tx);

        let config = PumpConfig {
            stage: PipelineStage::Transformer,
            set_length: None,
            expected_stops,
            boundary: ChannelBoundary::between(Stage::Transformer, Stage::Validator),
            env: env(cancel),
        };
        let rx = run_pump_stage(processor, in_rx, config);
        let output: Signals = rx.into_iter().collect();

        (batches, outcome, output)
    }

    /// Classifies an output signal so assertions read without matching the full enum each time.
    fn tag(signal: &Signal<Batch<u32>, PipelineError>) -> &'static str {
        match signal {
            Signal::Start => "start",
            Signal::Stop => "stop",
            Signal::Data(_) => "data",
            Signal::Error(_) => "error",
            Signal::Meta(_) => "meta",
        }
    }

    /// Every value that reached the output, flattened across batches in arrival order.
    fn emitted(output: &Signals) -> Vec<u32> {
        output
            .iter()
            .filter_map(|signal| match signal {
                Signal::Data(batch) => Some(batch.iter().copied()),
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn the_output_is_bracketed_by_start_and_stop() {
        let (_, outcome, output) = run(1, Arc::new(AtomicBool::new(false)), vec![data(&[1]), Signal::Stop]);

        assert_eq!(tag(output.first().unwrap()), "start");
        assert_eq!(tag(output.last().unwrap()), "stop");
        assert_eq!(*outcome.lock().unwrap(), Some(StreamOutcome::Clean));
    }

    #[test]
    fn each_batch_reaches_the_processor_whole_and_in_order() {
        let (batches, _, output) = run(1, Arc::new(AtomicBool::new(false)), vec![data(&[1, 2]), data(&[3, 4, 5]), Signal::Stop]);

        // The runner hands batches straight over: it never re-groups them.
        assert_eq!(*batches.lock().unwrap(), vec![vec![1, 2], vec![3, 4, 5]]);
        assert_eq!(emitted(&output), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn an_empty_batch_is_processed_without_being_treated_as_end_of_stream() {
        let (batches, outcome, output) = run(1, Arc::new(AtomicBool::new(false)), vec![data(&[]), data(&[7]), Signal::Stop]);

        assert_eq!(*batches.lock().unwrap(), vec![Vec::new(), vec![7]]);
        assert_eq!(emitted(&output), vec![7]);
        assert_eq!(*outcome.lock().unwrap(), Some(StreamOutcome::Clean));
    }

    #[test]
    fn a_stream_that_ends_without_a_stop_still_finalizes_cleanly() {
        let (batches, outcome, _) = run(1, Arc::new(AtomicBool::new(false)), vec![data(&[1, 2])]);

        assert_eq!(*batches.lock().unwrap(), vec![vec![1, 2]]);
        assert_eq!(*outcome.lock().unwrap(), Some(StreamOutcome::Clean));
    }

    #[test]
    fn a_forwarded_error_marks_the_stream_as_failed() {
        let (_, outcome, _) = run(
            1,
            Arc::new(AtomicBool::new(false)),
            vec![
                data(&[1]),
                Signal::Error(PipelineError::StagePanic {
                    stage: PipelineStage::Transformer,
                }),
                Signal::Stop,
            ],
        );

        assert_eq!(*outcome.lock().unwrap(), Some(StreamOutcome::Failed));
    }

    #[test]
    fn a_cancelled_stream_marks_the_stream_as_failed() {
        let (_, outcome, _) = run(1, Arc::new(AtomicBool::new(true)), vec![data(&[1]), data(&[2]), Signal::Stop]);

        assert_eq!(*outcome.lock().unwrap(), Some(StreamOutcome::Failed));
    }

    #[test]
    fn every_expected_stop_must_arrive_before_the_stage_finishes() {
        let (batches, _, _) = run(2, Arc::new(AtomicBool::new(false)), vec![data(&[1]), Signal::Stop, data(&[2]), Signal::Stop]);

        // The batch after the first `Stop` proves the stage kept pumping until the second arrived.
        assert_eq!(*batches.lock().unwrap(), vec![vec![1], vec![2]]);
    }

    #[test]
    fn a_raised_cancel_flag_short_circuits_processing() {
        let (batches, _, output) = run(1, Arc::new(AtomicBool::new(true)), vec![data(&[1]), data(&[2]), Signal::Stop]);

        // Cancellation is polled before the first batch, so nothing is processed, yet the stage still
        // brackets its output with `Start` and `Stop`.
        assert!(batches.lock().unwrap().is_empty());
        assert_eq!(tag(output.first().unwrap()), "start");
        assert_eq!(tag(output.last().unwrap()), "stop");
    }

    #[test]
    fn error_and_meta_signals_pass_straight_through() {
        let (_, _, output) = run(
            1,
            Arc::new(AtomicBool::new(false)),
            vec![
                Signal::Meta(MetaSignal::InputCount(7)),
                Signal::Error(PipelineError::StagePanic {
                    stage: PipelineStage::Transformer,
                }),
                Signal::Stop,
            ],
        );

        let tags: Vec<&str> = output.iter().map(tag).collect();
        assert_eq!(tags, vec!["start", "meta", "error", "stop"]);
    }

    #[test]
    fn a_batch_is_counted_as_received_once_per_item() {
        let telemetry = Arc::new(RunTelemetry::new());
        let stage_env = StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter: &NOOP as &'static dyn Reporter,
            controller: Arc::new(ToggleController {
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            telemetry: Arc::clone(&telemetry),
        };

        let (in_tx, in_rx) = channel(ChannelPolicy::Unbounded);
        in_tx.send(data(&[1, 2, 3])).unwrap();
        in_tx.send(data(&[4, 5])).unwrap();
        in_tx.send(Signal::Stop).unwrap();
        drop(in_tx);

        let processor = RecordingProcessor {
            batches: Arc::new(Mutex::new(Vec::new())),
            outcome: Arc::new(Mutex::new(None)),
        };
        let rx = run_pump_stage(
            processor,
            in_rx,
            PumpConfig {
                stage: PipelineStage::Transformer,
                set_length: None,
                expected_stops: 1,
                boundary: ChannelBoundary::between(Stage::Transformer, Stage::Validator),
                env: stage_env,
            },
        );
        let _drained: Signals = rx.into_iter().collect();

        let snapshot = telemetry
            .snapshots()
            .into_iter()
            .find(|snapshot| snapshot.stage == Stage::Transformer)
            .expect("the transformer stage is measured");
        assert_eq!(snapshot.received, 5);
    }
}

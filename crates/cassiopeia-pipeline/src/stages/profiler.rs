use crate::{error::PipelineError, pipeline_stage::PipelineStage};
use cassiopeia_collector::error::CollectorError;
use cassiopeia_common::{
    batch::Batch,
    channel::{ChannelPolicy, ChannelReceiver, ChannelSender, channel_with_telemetry},
    format::DataFormat,
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, payload::CollectedPayload};
use cassiopeia_profiler::{
    auto_profiler::AutoProfiler,
    passthrough_profiler::PassthroughProfiler,
    profiler::{Profiler, ProfilerRoutes},
};
use cassiopeia_reporter::reporter::Reporter;
use execution_time::ExecutionTime;
use std::{convert::Infallible, fs, sync::Arc, thread::spawn};

/// Spawns the profiler for one lane.
///
/// The profiler is handed the ingestor bank's route senders directly: both sides speak
/// [`ProfilerError`](cassiopeia_profiler::error::ProfilerError) over policy-neutral senders, so
/// routing inherits the pipeline's channel policy with no intermediate bridge thread. Each routed
/// payload is counted against the profiler stage after format detection and successful routing.
///
/// A profiler failure (a payload whose format cannot be detected, most commonly a source file that
/// cannot be read) is the lane's failure, so it is forwarded to the shared error path
/// (`error_sink`) as a fatal [`Signal::Error`] and counted against the run. This mirrors the
/// collector and ingestor lanes; without it a missing input would leave the run exiting cleanly.
pub(crate) fn spawn_profiler_thread(
    profiler: Box<dyn Profiler>,
    routes: ProfilerRoutes,
    error_sink: ChannelSender<Signal<Batch<Mapped<Fragment>>, PipelineError>>,
    telemetry: Arc<RunTelemetry>,
) {
    spawn(move || {
        if let Err(error) = profiler.profile(routes) {
            telemetry.add_errors(1);
            let _ = error_sink.send(Signal::Error(PipelineError::from(error)));
        }
    });
}

/// Builds the profiler for one lane, splitting the collector's stream onto the profiler's channel.
///
/// A declared format takes the passthrough profiler; an undeclared one takes the auto-detector. Data
/// payloads flow to the profiler on a policy-controlled channel; a collector failure is the lane's failure, so
/// it is forwarded straight to the shared error path (`error_sink`) and the lane is torn down,
/// rather than routed through the profiler as a re-typed error.
pub(crate) fn create_profiler(
    format_override: Option<DataFormat>,
    collector_rx: ChannelReceiver<Signal<CollectedPayload, CollectorError>>,
    channel_policy: ChannelPolicy,
    error_sink: ChannelSender<Signal<Batch<Mapped<Fragment>>, PipelineError>>,
    reporter: &'static dyn Reporter,
    telemetry: Arc<RunTelemetry>,
) -> Box<dyn Profiler> {
    let (profiler_tx, profiler_rx) = channel_with_telemetry::<Signal<CollectedPayload, Infallible>>(
        channel_policy,
        Some(Arc::clone(&telemetry)),
        Some(ChannelBoundary::between(Stage::Collector, Stage::Profiler)),
    );

    let splitter_telemetry = Arc::clone(&telemetry);
    spawn(move || {
        // This splitter is the collector pump's successor, so it owns the collector stage: it counts
        // every payload and its guard finishes once the collector's output is fully drained.
        let collector_stage = reporter
            .enter_stage(Box::new(PipelineStage::Collector), ExecutionTime::start())
            .with_telemetry(splitter_telemetry.start_stage(Stage::Collector));

        // An explicit receive rather than the channel's iterator, so the wait for the next payload
        // is measured as the collector stage's input wait.
        while let Ok(signal) = collector_stage.measure_input_wait(|| collector_rx.recv()) {
            match signal {
                Signal::Data(payload) => {
                    collector_stage.received(1);
                    collector_stage.inc_by(1);
                    splitter_telemetry.add_bytes_read(payload_bytes(&payload));
                    if profiler_tx.send(Signal::Data(payload)).is_err() {
                        break;
                    }
                }
                Signal::Error(error) => {
                    let _ = error_sink.send(Signal::Error(PipelineError::from(error)));
                    break;
                }
                Signal::Stop => {
                    let _ = profiler_tx.send(Signal::Stop);
                    break;
                }
                Signal::Meta(meta) => {
                    if profiler_tx.send(Signal::Meta(meta)).is_err() {
                        break;
                    }
                }
                Signal::Start => {}
            }
        }
    });

    match format_override {
        Some(format) => Box::new(PassthroughProfiler::new(format, profiler_rx, telemetry)),
        None => Box::new(AutoProfiler::new(profiler_rx, telemetry)),
    }
}

fn payload_bytes(payload: &CollectedPayload) -> u64 {
    match payload {
        CollectedPayload::Bytes(bytes) => u64::try_from(bytes.data().len()).unwrap_or(u64::MAX),
        CollectedPayload::File(file) => fs::metadata(file.path()).map_or(0, |metadata| metadata.len()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::PipelineError, stages::profiler::spawn_profiler_thread};
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        signal::Signal,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
    use cassiopeia_profiler::{
        error::ProfilerError,
        profiler::{Profiler, ProfilerRoutes},
    };
    use std::{collections::HashMap, sync::Arc};

    /// A profiler that fails as a missing or unreadable source file does: format detection cannot
    /// complete, so `profile` returns an error rather than routing anything.
    struct FailingProfiler;

    impl Profiler for FailingProfiler {
        fn profile(self: Box<Self>, _routes: ProfilerRoutes) -> Result<(), ProfilerError> {
            Err(ProfilerError::ChannelClosed)
        }
    }

    /// A profiler that completes without routing or failing, standing in for a clean lane.
    struct SilentProfiler;

    impl Profiler for SilentProfiler {
        fn profile(self: Box<Self>, _routes: ProfilerRoutes) -> Result<(), ProfilerError> {
            Ok(())
        }
    }

    #[test]
    fn a_profiler_failure_is_forwarded_to_the_shared_error_sink_and_counted() {
        let telemetry = Arc::new(RunTelemetry::new());
        let (error_tx, error_rx) = channel::<Signal<Batch<Mapped<Fragment>>, PipelineError>>(ChannelPolicy::Unbounded);

        spawn_profiler_thread(Box::new(FailingProfiler), HashMap::new(), error_tx, Arc::clone(&telemetry));

        let signal = error_rx.recv().expect("the profiler thread must forward its failure onto the error sink");
        assert!(matches!(signal, Signal::Error(PipelineError::Profiler(_))));
        assert_eq!(telemetry.snapshot().counters.errors, 1);
    }

    #[test]
    fn a_successful_profiler_forwards_no_error() {
        let telemetry = Arc::new(RunTelemetry::new());
        let (error_tx, error_rx) = channel::<Signal<Batch<Mapped<Fragment>>, PipelineError>>(ChannelPolicy::Unbounded);

        spawn_profiler_thread(Box::new(SilentProfiler), HashMap::new(), error_tx, Arc::clone(&telemetry));

        // The thread drops its sender when it ends, so a clean lane closes the channel with nothing
        // queued and no error counted.
        assert!(error_rx.recv().is_err());
        assert_eq!(telemetry.snapshot().counters.errors, 0);
    }
}

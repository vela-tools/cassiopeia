use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::ingestor_factories::{IngestorFactory, PreparedIngestor, ingestor_factories},
};
use cassiopeia_common::{
    batch::Batch,
    channel::{ChannelPolicy, ChannelReceiver, ChannelSender, channel_with_telemetry},
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry, stage_metrics::StageTelemetry},
};
use cassiopeia_ingestor::{error::IngestorError, ingestor::Ingestor};
use cassiopeia_ir::{payload::ProfiledPayload, record::Record};
use cassiopeia_manifest::{mapping_binding::MappingBinding, output::temporal::TemporalRepresentation};
use cassiopeia_profiler::{error::ProfilerError, profiler::ProfilerRoutes};
use cassiopeia_reporter::reporter::Reporter;
use execution_time::ExecutionTime;
use std::{collections::HashMap, sync::Arc, thread::spawn};

/// The pre-spawned bank of ingestor threads, one waiting on each format's input channel, all writing
/// records to one shared output channel.
pub(crate) struct IngestorBank {
    /// The policy-controlled input channel for each format's ingestor.
    pub(crate) routes: ProfilerRoutes,
    /// The shared policy-controlled output every ingestor writes records into.
    pub(crate) output_rx: ChannelReceiver<Signal<Batch<Record>, PipelineError>>,
}

/// Spawns one ingestor thread per supported format, each idle until a payload for its format arrives.
///
/// `mapping_binding` is the lane's binding, which decides how a JSON source is split into records and
/// which tables of a `GeoPackage` are read; `temporal` is the run's temporal output, which decides
/// which SensorThings snapshots are read.
pub(crate) fn spawn_ingestor_bank(
    mapping_binding: &MappingBinding,
    temporal: Option<TemporalRepresentation>,
    batch_size: usize,
    channel_policy: ChannelPolicy,
    reporter: &'static dyn Reporter,
    telemetry: &Arc<RunTelemetry>,
) -> IngestorBank {
    let (output_tx, output_rx) = channel_with_telemetry::<Signal<Batch<Record>, PipelineError>>(
        channel_policy,
        Some(Arc::clone(telemetry)),
        Some(ChannelBoundary::between(Stage::Ingestor, Stage::Expander)),
    );
    let mut routes: ProfilerRoutes = HashMap::new();

    for (format, factory) in ingestor_factories(mapping_binding, temporal) {
        let (ingestor_tx, ingestor_rx) = channel_with_telemetry::<Signal<ProfiledPayload, ProfilerError>>(
            channel_policy,
            Some(Arc::clone(telemetry)),
            Some(ChannelBoundary::between(Stage::Profiler, Stage::Ingestor)),
        );
        let shared_tx = output_tx.clone();
        let lane_telemetry = Arc::clone(telemetry);
        spawn(move || run_ingestor_lane(&factory, batch_size, channel_policy, ingestor_rx, &shared_tx, reporter, &lane_telemetry));
        routes.insert(format, ingestor_tx);
    }

    // Drop the original sender so the output receiver ends once every ingestor's clone is gone.
    drop(output_tx);

    IngestorBank { routes, output_rx }
}

/// Waits for payloads of one format, building and running an ingestor for each.
///
/// The stage guard finishes when the lane's input closes, after the last payload has been counted.
fn run_ingestor_lane(
    factory: &IngestorFactory,
    batch_size: usize,
    channel_policy: ChannelPolicy,
    ingestor_rx: ChannelReceiver<Signal<ProfiledPayload, ProfilerError>>,
    shared_tx: &ChannelSender<Signal<Batch<Record>, PipelineError>>,
    reporter: &'static dyn Reporter,
    telemetry: &RunTelemetry,
) {
    // The lane drives the profiler bar's count while detecting each payload's format, but the
    // profiler stage's throughput is measured where the profiling actually happens (the profiler's
    // `route_payloads`), so this bar carries no telemetry of its own and only advances its count.
    let profiler_stage = reporter.enter_stage(Box::new(PipelineStage::Profiler), ExecutionTime::start());
    let ingestor_telemetry = telemetry.start_stage(Stage::Ingestor);

    // An explicit receive rather than the channel's iterator, so the lane's wait for the next
    // profiled payload is measured as the ingestor stage's input wait.
    while let Ok(signal) = ingestor_telemetry.measure_input_wait(|| ingestor_rx.recv()) {
        match signal {
            Signal::Data(profiled) => {
                profiler_stage.inc_by(1);
                match factory(profiled, batch_size, reporter) {
                    Ok(PreparedIngestor {
                        ingestor,
                        warnings,
                        after_ingest,
                    }) => {
                        count_warnings(&ingestor_telemetry, telemetry, warnings);
                        let service = ingestor_telemetry.service_span();
                        ingest_one(ingestor, channel_policy, shared_tx, telemetry);
                        drop(service);
                        if let Some(after_ingest) = after_ingest {
                            count_warnings(&ingestor_telemetry, telemetry, after_ingest(reporter));
                        }
                    }
                    Err(error) => {
                        ingestor_telemetry.failed(1);
                        telemetry.add_errors(1);
                        let _ = shared_tx.send(Signal::Error(error));
                    }
                }
            }
            Signal::Stop => break,
            Signal::Error(error) => {
                let _ = shared_tx.send(Signal::Error(PipelineError::from(error)));
                break;
            }
            Signal::Meta(meta) => {
                let _ = shared_tx.send(Signal::Meta(meta));
            }
            Signal::Start => {}
        }
    }

    // The lane owns its input channel: releasing it here is what closes this format's route, so the
    // receiver is consumed rather than borrowed even though the loop only reads through it.
    drop(ingestor_rx);
}

/// Counts `warnings` against the ingestor stage and the run, when there are any.
fn count_warnings(stage: &StageTelemetry, run: &RunTelemetry, warnings: u64) {
    if warnings > 0 {
        stage.warning(warnings);
        run.add_warnings(warnings);
    }
}

/// Runs one ingestor to completion, forwarding its record batches onto the shared output.
///
/// This counts the run's input records but not the ingestor stage's completions: the ingestor stage
/// is measured where its records converge into the expander (`spawn_expander_thread`), so counting
/// them here as well would double the stage's tally.
fn ingest_one(ingestor: Box<dyn Ingestor>, channel_policy: ChannelPolicy, shared_tx: &ChannelSender<Signal<Batch<Record>, PipelineError>>, run: &RunTelemetry) {
    // The inner handoff follows the same run policy as every outer stage boundary.
    let (inner_tx, inner_rx) = channel_with_telemetry::<Signal<Vec<Record>, IngestorError>>(channel_policy, None, None);
    let handle = spawn(move || ingestor.ingest(inner_tx));

    for inner_signal in inner_rx {
        let count = match &inner_signal {
            Signal::Data(records) => u64::try_from(records.len()).unwrap_or(u64::MAX),
            Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => 0,
        };
        // An ingestor already reads its source in groups of the run's batch size, so its record
        // vector becomes the batch the rest of the pipeline moves without regrouping or copying.
        let outbound = inner_signal.map(Batch::from).map_err(PipelineError::from);
        if shared_tx.send(outbound).is_err() {
            break;
        }
        if count > 0 {
            run.add_input_records(count);
        }
    }

    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = shared_tx.send(Signal::Error(PipelineError::from(error)));
        }
        Err(_) => {
            let _ = shared_tx.send(Signal::Error(PipelineError::StagePanic {
                stage: PipelineStage::Ingestor,
            }));
        }
    }
}

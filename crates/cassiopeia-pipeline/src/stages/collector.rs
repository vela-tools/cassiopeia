use cassiopeia_collector::{collector::Collector, error::CollectorError};
use cassiopeia_common::{
    channel::{ChannelPolicy, ChannelReceiver, channel_with_telemetry},
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_ir::payload::CollectedPayload;
use std::{sync::Arc, thread::spawn};

/// Spawns the collector for one lane, returning the configured channel its payloads flow through.
///
/// The `Collector` trait is handed the stage channel directly, so the collector produces without an
/// intermediate pump thread and inherits the run's throughput or back-pressure policy. Its own
/// [`CollectorError`] travels on the channel; the consumer (the profiler splitter) counts each
/// payload against the collector stage and bridges the error to the pipeline error type.
pub(crate) fn spawn_collector_thread(
    collector: Box<dyn Collector>,
    channel_policy: ChannelPolicy,
    telemetry: Arc<RunTelemetry>,
) -> ChannelReceiver<Signal<CollectedPayload, CollectorError>> {
    let (tx, rx) = channel_with_telemetry(
        channel_policy,
        Some(Arc::clone(&telemetry)),
        Some(ChannelBoundary::between(Stage::Collector, Stage::Profiler)),
    );

    spawn(move || {
        let stage = telemetry.start_stage(Stage::Collector);
        if tx.send(Signal::Start).is_err() {
            return;
        }

        let service = stage.service_span();
        if let Err(error) = collector.collect(tx.clone()) {
            stage.failed(1);
            telemetry.add_errors(1);
            let _ = tx.send(Signal::Error(error));
        }
        drop(service);

        let _ = tx.send(Signal::Stop);
    });

    rx
}

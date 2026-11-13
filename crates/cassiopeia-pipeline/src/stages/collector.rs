use crate::{input_config::LaneSource, stages::limits_reached_report::report_limits_reached};
use cassiopeia_collector::{
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    generic::GenericCollector,
    sensorthings::collector::SensorThingsCollector,
};
use cassiopeia_common::{
    channel::{ChannelPolicy, ChannelReceiver, channel_with_telemetry},
    cycle_start::CycleStart,
    format::DataFormat,
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_ir::payload::CollectedPayload;
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::{
    sync::{Arc, atomic::AtomicBool},
    thread::spawn,
};

/// The collector that reads a lane's `source` this cycle, fetching through `downloader`.
///
/// Files are read as they are, tagged with the lane's declared format. A SensorThings server is read
/// into a snapshot built from `cycle_start`, so every attempt at one scheduled run asks the server for
/// the same thing; both stop once `shutdown` is set.
pub(crate) fn lane_collector(
    source: &LaneSource,
    format_override: Option<DataFormat>,
    cycle_start: CycleStart,
    downloader: Downloader,
    shutdown: &'static AtomicBool,
) -> Box<dyn Collector> {
    match source {
        // The lane configuration is borrowed and reused every cycle, so each collector owns a copy.
        LaneSource::Files(files) => Box::new(GenericCollector::new(files.clone(), format_override, downloader)),
        LaneSource::SensorThings(service) => Box::new(SensorThingsCollector::new(service.clone(), cycle_start, downloader, shutdown)),
    }
}

/// Spawns the collector for one lane, returning the configured channel its payloads flow through.
///
/// The `Collector` trait is handed the stage channel directly, so the collector produces without an
/// intermediate pump thread and inherits the run's throughput or back-pressure policy. Its own
/// [`CollectorError`] travels on the channel; the consumer (the profiler splitter) counts each
/// payload against the collector stage and bridges the error to the pipeline error type. What the
/// collector met that the run carries on past is reported to `sink` once it has collected, and
/// counted among the run's warnings.
pub(crate) fn spawn_collector_thread(
    collector: Box<dyn Collector>,
    channel_policy: ChannelPolicy,
    sink: &'static dyn DiagnosticSink,
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
        match collector.collect(tx.clone()) {
            Ok(warnings) => telemetry.add_warnings(report_limits_reached(sink, &warnings)),
            Err(error) => {
                stage.failed(1);
                telemetry.add_errors(1);
                let _ = tx.send(Signal::Error(error));
            }
        }
        drop(service);

        let _ = tx.send(Signal::Stop);
    });

    rx
}

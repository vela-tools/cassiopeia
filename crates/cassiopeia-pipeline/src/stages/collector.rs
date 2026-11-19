use crate::{
    input_config::LaneSource,
    stages::{
        empty_ranges_report::report_empty_ranges,
        limits_reached_report::report_limits_reached,
        missing_datastreams_report::report_missing_datastreams,
        unread_multi_datastreams_report::report_unread_multi_datastreams,
        unserved_features_of_interest_report::report_unserved_features_of_interest,
    },
};
use cassiopeia_collector::{
    collection_end::CollectionEnd,
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    generic::GenericCollector,
    scratch_files::ScratchFiles,
    sensorthings::{collector::SensorThingsCollector, cursor_cycle::CursorCycle, unread_check::UnreadCheck},
};
use cassiopeia_common::{
    channel::{ChannelPolicy, ChannelReceiver, channel},
    cycle_start::CycleStart,
    format::DataFormat,
    signal::Signal,
    stage::Stage,
    telemetry::run::RunTelemetry,
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
/// the same thing, an incremental read resumes from `cursor`, and a server is checked for
/// `MultiDatastreams` its input reads none of when `unread_check` is due; both stop once `shutdown`
/// is set.
pub(crate) fn lane_collector(
    source: &LaneSource,
    cursor: Option<CursorCycle>,
    unread_check: UnreadCheck,
    format_override: Option<DataFormat>,
    cycle_start: CycleStart,
    downloader: Downloader,
    shutdown: &'static AtomicBool,
) -> Box<dyn Collector> {
    match source {
        // The lane configuration is borrowed and reused every cycle, so each collector owns a copy.
        LaneSource::Files(files) => Box::new(GenericCollector::new(files.clone(), format_override, downloader)),
        LaneSource::SensorThings(service) => Box::new(SensorThingsCollector::new(
            service.clone(),
            cursor,
            unread_check,
            cycle_start,
            downloader,
            shutdown,
        )),
    }
}

/// Spawns the collector for one lane, writing what it downloads to `scratch`, returning the
/// configured channel its payloads flow through.
///
/// The `Collector` trait is handed the stage channel directly, so the collector produces without an
/// intermediate pump thread and inherits the run's throughput or back-pressure policy. Its own
/// [`CollectorError`] travels on the channel; the consumer (the profiler splitter) counts each
/// payload against the collector stage and bridges the error to the pipeline error type. What the
/// collector met that the run carries on past is reported to `sink` once it has collected, and
/// counted among the run's warnings. A collection the run's stop ended counts neither: the cycle
/// reports itself stopped.
pub(crate) fn spawn_collector_thread(
    collector: Box<dyn Collector>,
    scratch: ScratchFiles,
    channel_policy: ChannelPolicy,
    sink: &'static dyn DiagnosticSink,
    telemetry: Arc<RunTelemetry>,
) -> ChannelReceiver<Signal<CollectedPayload, CollectorError>> {
    // The hop to the splitter stays inside the collector stage: the splitter's own channel is the
    // boundary to the profiler, and it alone is reported as one.
    let (tx, rx) = channel(channel_policy);

    spawn(move || {
        let stage = telemetry.start_stage(Stage::Collector);
        if tx.send(Signal::Start).is_err() {
            return;
        }

        let service = stage.service_span();
        match collector.collect(scratch, tx.clone()) {
            Ok(CollectionEnd::Collected(warnings)) => telemetry.add_warnings(
                report_limits_reached(sink, &warnings)
                    + report_empty_ranges(sink, &warnings)
                    + report_missing_datastreams(sink, &warnings)
                    + report_unread_multi_datastreams(sink, &warnings)
                    + report_unserved_features_of_interest(sink, &warnings),
            ),
            Ok(CollectionEnd::Stopped) => {}
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

#[cfg(test)]
mod tests {
    use crate::stages::collector::spawn_collector_thread;
    use cassiopeia_collector::{
        collection_end::CollectionEnd,
        collection_warnings::CollectionWarnings,
        collector::Collector,
        error::CollectorError,
        scratch_files::ScratchFiles,
    };
    use cassiopeia_common::{
        channel::{ChannelPolicy, ChannelSender},
        signal::Signal,
        telemetry::{channel_metrics::ChannelSnapshot, run::RunTelemetry},
    };
    use cassiopeia_ir::payload::CollectedPayload;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use pretty_assertions::assert_eq;
    use std::{path::Path, sync::Arc};

    /// A collector with no input, which ends at once having collected nothing.
    struct EmptyCollector;

    impl Collector for EmptyCollector {
        fn collect(
            self: Box<Self>,
            _scratch: ScratchFiles,
            _sender: ChannelSender<Signal<CollectedPayload, CollectorError>>,
        ) -> Result<CollectionEnd, CollectorError> {
            Ok(CollectionEnd::Collected(CollectionWarnings::default()))
        }
    }

    static SINK: NoopReporter = NoopReporter::new();

    #[test]
    fn the_hop_from_the_collector_to_its_splitter_is_not_reported_as_a_channel() {
        let telemetry = Arc::new(RunTelemetry::new());

        let receiver = spawn_collector_thread(
            Box::new(EmptyCollector),
            ScratchFiles::new(Arc::from(Path::new("scratch")), 0),
            ChannelPolicy::Unbounded,
            &SINK,
            Arc::clone(&telemetry),
        );
        drop(receiver);

        assert_eq!(telemetry.snapshot().channels, Vec::<ChannelSnapshot>::new());
    }
}

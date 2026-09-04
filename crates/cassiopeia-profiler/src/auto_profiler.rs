use crate::{
    error::ProfilerError,
    profiler::{Profiler, ProfilerRoutes},
    routing::route_payloads,
};
use cassiopeia_common::{channel::ChannelReceiver, signal::Signal, telemetry::run::RunTelemetry};
use cassiopeia_data_profiler::{profile_bytes, profile_path};
use cassiopeia_ir::payload::CollectedPayload;
use std::{convert::Infallible, sync::Arc};

/// A profiler that detects each payload's format from its content.
///
/// Used when no format is declared for an input, so every payload is run through the
/// [`cassiopeia_data_profiler`] detectors before it is routed.
pub struct AutoProfiler {
    receiver: ChannelReceiver<Signal<CollectedPayload, Infallible>>,
    telemetry: Arc<RunTelemetry>,
}

impl AutoProfiler {
    /// Builds an auto-detecting profiler that reads collected payloads from `receiver`.
    #[must_use]
    pub const fn new(receiver: ChannelReceiver<Signal<CollectedPayload, Infallible>>, telemetry: Arc<RunTelemetry>) -> AutoProfiler {
        AutoProfiler { receiver, telemetry }
    }
}

impl Profiler for AutoProfiler {
    fn profile(self: Box<Self>, routes: ProfilerRoutes) -> Result<(), ProfilerError> {
        route_payloads(
            &self.receiver,
            routes,
            |payload| match payload {
                CollectedPayload::File(file) => Ok(profile_path(file.path())?),
                CollectedPayload::Bytes(bytes) => Ok(profile_bytes(bytes.data())?),
            },
            self.telemetry.as_ref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        auto_profiler::AutoProfiler,
        error::ProfilerError,
        profiler::{Profiler, ProfilerRoutes},
    };
    use cassiopeia_common::{
        channel::{ChannelPolicy, channel},
        format::DataFormat,
        signal::Signal,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_data_profiler::metadata::FormatMetadata;
    use cassiopeia_ir::payload::{BytesPayload, CollectedPayload};
    use std::sync::Arc;

    fn csv_bytes_payload() -> CollectedPayload {
        CollectedPayload::Bytes(BytesPayload::new(b"id,name\n1,alpha\n2,beta\n".to_vec(), None))
    }

    #[test]
    fn a_csv_payload_is_detected_and_routed_to_the_csv_ingestor() {
        let (input_tx, input_rx) = channel(ChannelPolicy::Unbounded);
        input_tx.send(Signal::Data(csv_bytes_payload())).unwrap();
        input_tx.send(Signal::Stop).unwrap();
        drop(input_tx);

        let (csv_tx, csv_rx) = channel(ChannelPolicy::Unbounded);
        let mut routes = ProfilerRoutes::new();
        routes.insert(DataFormat::Csv, csv_tx);

        Box::new(AutoProfiler::new(input_rx, Arc::new(RunTelemetry::new()))).profile(routes).unwrap();

        match csv_rx.recv().unwrap() {
            Signal::Data(profiled) => {
                assert_eq!(*profiled.profile().format(), DataFormat::Csv);
                let metadata = profiled.profile().metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();
                assert_eq!(metadata.dialect().delimiter, b',');
            }
            other @ (Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_)) => {
                panic!("expected a data signal, got {other:?}")
            }
        }
    }

    #[test]
    fn a_detected_format_with_no_registered_route_is_an_error() {
        let (input_tx, input_rx) = channel(ChannelPolicy::Unbounded);
        input_tx.send(Signal::Data(csv_bytes_payload())).unwrap();
        input_tx.send(Signal::Stop).unwrap();
        drop(input_tx);

        // The route table only knows JSON, so a detected CSV payload has nowhere to go.
        let (json_tx, _json_rx) = channel(ChannelPolicy::Unbounded);
        let mut routes = ProfilerRoutes::new();
        routes.insert(DataFormat::Json, json_tx);

        let result = Box::new(AutoProfiler::new(input_rx, Arc::new(RunTelemetry::new()))).profile(routes);

        assert!(matches!(result, Err(ProfilerError::NoRoute { format: DataFormat::Csv, .. })));
    }
}

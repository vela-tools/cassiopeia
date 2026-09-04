use crate::{
    error::ProfilerError,
    profiler::{Profiler, ProfilerRoutes},
    routing::route_payloads,
};
use cassiopeia_common::{
    channel::ChannelReceiver,
    error::io::{IoAction, IoError},
    format::DataFormat,
    signal::Signal,
    telemetry::run::RunTelemetry,
};
use cassiopeia_data_profiler::{error::DataProfilerError, inspectors::inspect, profile::Profile};
use cassiopeia_ir::payload::CollectedPayload;
use mediatype::{MediaType, media_type};
use std::{convert::Infallible, fs, sync::Arc};

/// The MIME type recorded for a declared format, whose bytes were never inspected for a real one.
const DECLARED_MIME: MediaType<'static> = media_type!(APPLICATION / OCTET_STREAM);

/// A profiler that trusts a format the caller already declared, skipping format *guessing* only.
///
/// The declared format is accepted as-is, but the format's structural inspector still runs, so a
/// declared CSV keeps its real dialect and a declared GRIB keeps its edition, the same metadata the
/// auto-detect path attaches. Every payload is routed straight to that format's ingestor with a
/// maximal-confidence profile.
pub struct PassthroughProfiler {
    format: DataFormat,
    receiver: ChannelReceiver<Signal<CollectedPayload, Infallible>>,
    telemetry: Arc<RunTelemetry>,
}

impl PassthroughProfiler {
    /// Builds a passthrough profiler that routes every payload as `format`.
    #[must_use]
    pub const fn new(format: DataFormat, receiver: ChannelReceiver<Signal<CollectedPayload, Infallible>>, telemetry: Arc<RunTelemetry>) -> PassthroughProfiler {
        PassthroughProfiler { format, receiver, telemetry }
    }
}

impl Profiler for PassthroughProfiler {
    fn profile(self: Box<Self>, routes: ProfilerRoutes) -> Result<(), ProfilerError> {
        let format = self.format;
        route_payloads(&self.receiver, routes, move |payload| profile_payload(format, payload), self.telemetry.as_ref())
    }
}

/// Builds the profile for one declared-format payload, running the format's inspector over its bytes.
///
/// The bytes are read the same way the auto-detect path reads them: a file payload is read from disk, a
/// bytes payload is used directly. An inspector error is propagated as the lane's error, so a malformed
/// declared file fails rather than silently producing an uninspected profile.
fn profile_payload(format: DataFormat, payload: &CollectedPayload) -> Result<Profile, ProfilerError> {
    let profile = Profile::new(format, DECLARED_MIME, 1.0);
    let metadata = match payload {
        CollectedPayload::File(file) => {
            let bytes = fs::read(file.path()).map_err(|source| {
                DataProfilerError::from(IoError::FileOperation {
                    source,
                    path: file.path().clone(),
                    action: IoAction::Read,
                })
            })?;
            inspect(format, &bytes)?
        }
        CollectedPayload::Bytes(bytes) => inspect(format, bytes.data())?,
    };
    Ok(match metadata {
        Some(metadata) => profile.with_metadata(metadata),
        None => profile,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        passthrough_profiler::PassthroughProfiler,
        profiler::{Profiler, ProfilerRoutes},
    };
    use cassiopeia_common::{
        channel::{ChannelPolicy, channel},
        format::DataFormat,
        signal::Signal,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_data_profiler::metadata::FormatMetadata;
    use cassiopeia_ir::payload::{BytesPayload, CollectedPayload, ProfiledPayload};
    use std::sync::Arc;

    /// Routes one in-memory payload through a passthrough profiler for `format` and returns the profiled
    /// payload it emits.
    fn profile_one(format: DataFormat, data: Vec<u8>) -> ProfiledPayload {
        let (input_tx, input_rx) = channel(ChannelPolicy::Unbounded);
        input_tx.send(Signal::Data(CollectedPayload::Bytes(BytesPayload::new(data, None)))).unwrap();
        input_tx.send(Signal::Stop).unwrap();
        drop(input_tx);

        let (route_tx, route_rx) = channel(ChannelPolicy::Unbounded);
        let mut routes = ProfilerRoutes::new();
        routes.insert(format, route_tx);

        Box::new(PassthroughProfiler::new(format, input_rx, Arc::new(RunTelemetry::new())))
            .profile(routes)
            .unwrap();

        match route_rx.recv().unwrap() {
            Signal::Data(profiled) => profiled,
            other @ (Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_)) => {
                panic!("expected a data signal, got {other:?}")
            }
        }
    }

    #[test]
    fn a_declared_format_routes_to_its_own_ingestor() {
        let profiled = profile_one(DataFormat::Json, b"content the detector would never recognise".to_vec());
        assert_eq!(*profiled.profile().format(), DataFormat::Json);
    }

    #[test]
    fn a_declared_csv_keeps_its_inspected_dialect() {
        let profiled = profile_one(DataFormat::Csv, b"id;name\n1;alpha\n2;beta\n".to_vec());
        assert_eq!(*profiled.profile().format(), DataFormat::Csv);
        let metadata = profiled.profile().metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();
        assert_eq!(metadata.dialect().delimiter, b';');
    }

    #[test]
    fn a_declared_grib_keeps_its_inspected_edition() {
        let profiled = profile_one(DataFormat::Grib, b"GRIB\x00\x00\x00\x02\x00\x00\x00\x00".to_vec());
        assert_eq!(*profiled.profile().format(), DataFormat::Grib);
        assert!(profiled.profile().metadata().as_ref().and_then(FormatMetadata::as_grib).is_some());
    }
}

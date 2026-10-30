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
use cassiopeia_csv_dialect::statement::DialectStatement;
use cassiopeia_data_profiler::{content::Content, error::DataProfilerError, inspectors::inspect, profile::Profile};
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
///
/// A declared CSV is inspected under the dialect its manifest input states, so the stated members are
/// taken as stated and only the rest are detected.
pub struct PassthroughProfiler {
    format: DataFormat,
    dialect: DialectStatement,
    receiver: ChannelReceiver<Signal<CollectedPayload, Infallible>>,
    telemetry: Arc<RunTelemetry>,
}

impl PassthroughProfiler {
    /// Builds a passthrough profiler that routes every payload as `format`, inspecting a CSV under
    /// the stated `dialect`.
    #[must_use]
    pub const fn new(
        format: DataFormat,
        dialect: DialectStatement,
        receiver: ChannelReceiver<Signal<CollectedPayload, Infallible>>,
        telemetry: Arc<RunTelemetry>,
    ) -> PassthroughProfiler {
        PassthroughProfiler {
            format,
            dialect,
            receiver,
            telemetry,
        }
    }
}

impl Profiler for PassthroughProfiler {
    fn profile(self: Box<Self>, routes: ProfilerRoutes) -> Result<(), ProfilerError> {
        let format = self.format;
        let dialect = &self.dialect;
        route_payloads(
            &self.receiver,
            routes,
            |payload| profile_payload(format, dialect, payload),
            self.telemetry.as_ref(),
        )
    }
}

/// Builds the profile for one declared-format payload, running the format's inspector over its bytes.
///
/// The bytes are read the same way the auto-detect path reads them: a file payload is read from disk, a
/// bytes payload is used directly. An inspector error is propagated as the lane's error, so a malformed
/// declared file fails rather than silently producing an uninspected profile.
fn profile_payload(format: DataFormat, dialect: &DialectStatement, payload: &CollectedPayload) -> Result<Profile, ProfilerError> {
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
            inspect(format, &Content::hinted(&bytes, file.encoding_hint()), dialect)?
        }
        CollectedPayload::Bytes(bytes) => inspect(format, &Content::hinted(bytes.data(), bytes.encoding_hint()), dialect)?,
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
    use cassiopeia_csv_dialect::{dialect_byte::DialectByte, preamble::Preamble, statement::DialectStatement};
    use cassiopeia_data_profiler::{encoding_hint::EncodingHint, metadata::FormatMetadata};
    use cassiopeia_ir::payload::{BytesPayload, CollectedPayload, ProfiledPayload};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    /// Routes one in-memory payload through a passthrough profiler for `format` and `dialect` and returns
    /// the profiled payload it emits.
    fn profile_one(format: DataFormat, dialect: DialectStatement, data: Vec<u8>) -> ProfiledPayload {
        let (input_tx, input_rx) = channel(ChannelPolicy::Unbounded);
        input_tx
            .send(Signal::Data(CollectedPayload::Bytes(BytesPayload::new(data, None, EncodingHint::NONE))))
            .unwrap();
        input_tx.send(Signal::Stop).unwrap();
        drop(input_tx);

        let (route_tx, route_rx) = channel(ChannelPolicy::Unbounded);
        let mut routes = ProfilerRoutes::new();
        routes.insert(format, route_tx);

        Box::new(PassthroughProfiler::new(format, dialect, input_rx, Arc::new(RunTelemetry::new())))
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
        let profiled = profile_one(
            DataFormat::Json,
            DialectStatement::NOTHING,
            b"content the detector would never recognise".to_vec(),
        );
        assert_eq!(*profiled.profile().format(), DataFormat::Json);
    }

    #[test]
    fn a_declared_csv_keeps_its_inspected_dialect() {
        let profiled = profile_one(DataFormat::Csv, DialectStatement::NOTHING, b"id;name\n1;alpha\n2;beta\n".to_vec());
        assert_eq!(*profiled.profile().format(), DataFormat::Csv);
        let metadata = profiled.profile().metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();
        assert_eq!(metadata.dialect().delimiter, DialectByte::SEMICOLON);
    }

    #[test]
    fn a_declared_csv_is_inspected_under_its_stated_dialect() {
        let dialect = serde_json::from_str(r#"{"skipRows": 1}"#).unwrap();
        let profiled = profile_one(DataFormat::Csv, dialect, b"Report;Vienna\nid;name\n1;alpha\n2;beta\n".to_vec());
        let metadata = profiled.profile().metadata().as_ref().and_then(FormatMetadata::as_csv).unwrap();

        assert_eq!(metadata.dialect().preamble, Preamble::new(1));
    }

    #[test]
    fn a_declared_grib_keeps_its_inspected_edition() {
        let profiled = profile_one(DataFormat::Grib, DialectStatement::NOTHING, b"GRIB\x00\x00\x00\x02\x00\x00\x00\x00".to_vec());
        assert_eq!(*profiled.profile().format(), DataFormat::Grib);
        assert!(profiled.profile().metadata().as_ref().and_then(FormatMetadata::as_grib).is_some());
    }
}

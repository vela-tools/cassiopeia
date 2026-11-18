//! What the SensorThings collector's tests share: a collector of a mock server, run to its end, and
//! the snapshot it wrote.

use crate::{
    collection_end::CollectionEnd,
    collection_warnings::CollectionWarnings,
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    scratch_files::ScratchFiles,
    sensorthings::{collector::SensorThingsCollector, cursor_cycle::CursorCycle, unread_check::UnreadCheck},
};
use cassiopeia_common::{
    channel::{ChannelPolicy, channel},
    cycle_start::CycleStart,
    request_header::RequestHeaders,
    signal::Signal,
    user_agent::UserAgent,
};
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use cassiopeia_sensorthings::{service::SensorThingsService, service_root::ServiceRoot};
use chrono::DateTime;
use mockito::ServerGuard;
use serde_json::Value;
use std::{fs, sync::atomic::AtomicBool};
use tempfile::TempDir;
use url::Url;

/// A shutdown flag never set, as in a run nobody stops.
pub(crate) static RUNNING: AtomicBool = AtomicBool::new(false);

/// The service root of the mock server, `{server}/v1.1`.
pub(crate) fn root(server: &ServerGuard) -> ServiceRoot {
    ServiceRoot::try_from(Url::parse(&format!("{}/v1.1", server.url())).unwrap()).unwrap()
}

/// The start of the cycle every test collects in, 2026-11-12T10:00:00Z.
pub(crate) fn cycle_start() -> CycleStart {
    CycleStart::at(DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().to_utc())
}

/// A collector of the mock server's root with the settings `settings` and the request headers
/// `headers`, in a cycle after the run's check for unread `MultiDatastreams`, that stops once
/// `shutdown` is set.
pub(crate) fn collector(server: &ServerGuard, settings: &str, headers: RequestHeaders, shutdown: &'static AtomicBool) -> Box<SensorThingsCollector> {
    let downloader = Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), headers, shutdown);
    let service = SensorThingsService::new(root(server), serde_json::from_str(settings).unwrap());
    Box::new(SensorThingsCollector::new(
        service,
        None,
        UnreadCheck::Done,
        cycle_start(),
        downloader,
        shutdown,
    ))
}

/// A collector of the mock server's root with the settings `settings`, in the cycle that checks for
/// unread `MultiDatastreams`, that nobody stops.
pub(crate) fn checking_collector(server: &ServerGuard, settings: &str) -> Box<SensorThingsCollector> {
    let downloader = Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING);
    let service = SensorThingsService::new(root(server), serde_json::from_str(settings).unwrap());
    Box::new(SensorThingsCollector::new(service, None, UnreadCheck::Due, cycle_start(), downloader, &RUNNING))
}

/// A collector of the mock server's root with the settings `settings`, resuming from `cursor`, that
/// nobody stops.
pub(crate) fn resuming_collector(server: &ServerGuard, settings: &str, cursor: CursorCycle) -> Box<SensorThingsCollector> {
    let downloader = Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING);
    let service = SensorThingsService::new(root(server), serde_json::from_str(settings).unwrap());
    Box::new(SensorThingsCollector::new(
        service,
        Some(cursor),
        UnreadCheck::Done,
        cycle_start(),
        downloader,
        &RUNNING,
    ))
}

/// Runs `collector` in a scratch folder of its own, returning how its collection ended, every
/// payload it sent, and the scratch folder, which holds what it wrote until it is dropped.
pub(crate) fn collect_to_end(collector: Box<SensorThingsCollector>) -> (Result<CollectionEnd, CollectorError>, Vec<FilePayload>, TempDir) {
    let scratch = TempDir::new().unwrap();
    let (sender, receiver) = channel(ChannelPolicy::Unbounded);
    let result = collector.collect(ScratchFiles::new(scratch.path().into(), 0), sender);
    let payloads = receiver
        .iter()
        .filter_map(|signal| match signal {
            Signal::Data(CollectedPayload::File(payload)) => Some(payload),
            Signal::Data(CollectedPayload::Bytes(_)) | Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
        })
        .collect();
    (result, payloads, scratch)
}

/// Runs `collector`, which nobody stops, in a scratch folder of its own, returning the warnings it
/// collected with, or how it failed, every payload it sent, and the scratch folder, which holds what
/// it wrote until it is dropped.
pub(crate) fn collect(collector: Box<SensorThingsCollector>) -> (Result<CollectionWarnings, CollectorError>, Vec<FilePayload>, TempDir) {
    let (result, payloads, scratch) = collect_to_end(collector);
    let warnings = result.map(|end| match end {
        CollectionEnd::Collected(warnings) => warnings,
        CollectionEnd::Stopped => panic!("a collector nobody stops was stopped"),
    });
    (warnings, payloads, scratch)
}

/// The lines of the one snapshot `payloads` holds, each read as JSON.
pub(crate) fn snapshot_lines(payloads: Vec<FilePayload>) -> Vec<Value> {
    let [payload] = <[FilePayload; 1]>::try_from(payloads).unwrap();
    let text = fs::read_to_string(payload.path()).unwrap();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

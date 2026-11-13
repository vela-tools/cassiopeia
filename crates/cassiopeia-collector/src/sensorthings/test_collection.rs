//! What the SensorThings collector's tests share: a collector of a mock server, run to its end, and
//! the snapshot it wrote.

use crate::{
    collection_warnings::CollectionWarnings,
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    sensorthings::{collector::SensorThingsCollector, cursor_cycle::CursorCycle},
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
/// `headers`, that stops once `shutdown` is set.
pub(crate) fn collector(server: &ServerGuard, settings: &str, headers: RequestHeaders, shutdown: &'static AtomicBool) -> Box<SensorThingsCollector> {
    let downloader = Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), headers, shutdown);
    let service = SensorThingsService::new(root(server), serde_json::from_str(settings).unwrap());
    Box::new(SensorThingsCollector::new(service, None, cycle_start(), downloader, shutdown))
}

/// A collector of the mock server's root with the settings `settings`, resuming from `cursor`, that
/// nobody stops.
pub(crate) fn resuming_collector(server: &ServerGuard, settings: &str, cursor: CursorCycle) -> Box<SensorThingsCollector> {
    let downloader = Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING);
    let service = SensorThingsService::new(root(server), serde_json::from_str(settings).unwrap());
    Box::new(SensorThingsCollector::new(service, Some(cursor), cycle_start(), downloader, &RUNNING))
}

/// Runs `collector`, returning what it returned and every payload it sent.
pub(crate) fn collect(collector: Box<SensorThingsCollector>) -> (Result<CollectionWarnings, CollectorError>, Vec<FilePayload>) {
    let (sender, receiver) = channel(ChannelPolicy::Unbounded);
    let result = collector.collect(sender);
    let payloads = receiver
        .iter()
        .filter_map(|signal| match signal {
            Signal::Data(CollectedPayload::File(payload)) => Some(payload),
            Signal::Data(CollectedPayload::Bytes(_)) | Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
        })
        .collect();
    (result, payloads)
}

/// The lines of the one snapshot `payloads` holds, each read as JSON, removing the snapshot once it
/// is read.
pub(crate) fn snapshot_lines(payloads: Vec<FilePayload>) -> Vec<Value> {
    let [payload] = <[FilePayload; 1]>::try_from(payloads).unwrap();
    let path = payload.into_path();
    let text = fs::read_to_string(&path).unwrap();
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

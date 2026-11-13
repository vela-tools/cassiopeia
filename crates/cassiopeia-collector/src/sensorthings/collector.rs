use crate::{
    collection_warnings::CollectionWarnings,
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    sensorthings::{
        cursor_cycle::CursorCycle,
        incremental_read::read_incremental,
        page_reader::PageReader,
        range_read::read_range,
        snapshot_failure::unwritable,
        traversal::{TraversalEnd, traverse},
    },
};
use cassiopeia_common::{channel::ChannelSender, cycle_start::CycleStart, format::DataFormat, masked_url::MaskedUrl, signal::Signal};
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use cassiopeia_sensorthings::{
    request::discovery_request::latest_discovery_request,
    service::SensorThingsService,
    settings::read_mode::ReadMode,
    snapshot::{header::SnapshotHeader, line::DatastreamLine, writer::SnapshotWriter},
};
use std::{
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    ops::ControlFlow,
    path::Path,
    sync::atomic::AtomicBool,
};
use temp_dir::TempDir;
use tracing::debug;

/// Collects one cycle of a SensorThings API server into a snapshot file and hands the snapshot on as
/// a downloaded `sensorthings` file, whose source is the service root.
///
/// From there the snapshot is profiled and ingested as a local one would be, so a read from a server
/// and a replay of its snapshot give the same records.
pub struct SensorThingsCollector {
    /// The server and how it is read.
    service: SensorThingsService,
    /// The cursor positions an incremental read resumes from and the slot it leaves its update in;
    /// `None` for any other read.
    cursor: Option<CursorCycle>,
    /// The instant the cycle started, which the snapshot records as its collection time.
    cycle_start: CycleStart,
    /// The downloader every page is fetched through, with the input's headers.
    downloader: Downloader,
    /// The process-wide shutdown flag, set while the run is being stopped.
    shutdown: &'static AtomicBool,
}

impl SensorThingsCollector {
    /// A collector of `service` for the cycle that started at `cycle_start`, resuming an incremental
    /// read from `cursor`, fetching through `downloader` and stopping once `shutdown` is set.
    #[must_use]
    pub const fn new(
        service: SensorThingsService,
        cursor: Option<CursorCycle>,
        cycle_start: CycleStart,
        downloader: Downloader,
        shutdown: &'static AtomicBool,
    ) -> SensorThingsCollector {
        SensorThingsCollector {
            service,
            cursor,
            cycle_start,
            downloader,
            shutdown,
        }
    }
}

impl Collector for SensorThingsCollector {
    fn collect(self: Box<Self>, sender: ChannelSender<Signal<CollectedPayload, CollectorError>>) -> Result<CollectionWarnings, CollectorError> {
        let SensorThingsCollector {
            service,
            cursor,
            cycle_start,
            downloader,
            shutdown,
        } = *self;
        let directory = TempDir::new().map_err(|source| CollectorError::Io { source, path: env::temp_dir() })?;
        // The snapshot must outlive the collector, because the profiler and the ingestor read it
        // later; cleanup is the caller's responsibility, as for every downloaded source.
        let directory = directory.dont_delete_on_drop();
        let path = directory.path().join(format!("snapshot.{}", DataFormat::SensorThings.extension()));

        let warnings = match write_snapshot(&service, cursor, cycle_start, downloader, shutdown, &path) {
            Ok(warnings) => warnings,
            Err(error) => {
                // Nothing reads a snapshot whose read failed, so it does not outlive the collector.
                if let Err(cleanup) = fs::remove_dir_all(directory.path()) {
                    debug!("The partial snapshot in {} could not be removed: {cleanup}", directory.path().display());
                }
                return Err(error);
            }
        };

        let source = MaskedUrl::plain(service.into_root().into());
        let payload = FilePayload::downloaded(path, source, Some(DataFormat::SensorThings), EncodingHint::NONE);
        sender
            .send(Signal::Data(CollectedPayload::File(payload)))
            .map_err(|_| CollectorError::ChannelClosed)?;
        Ok(warnings)
    }
}

/// Reads one cycle of `service` into a snapshot at `path`, recording `cycle_start` as its
/// collection time, resuming an incremental read from `cursor`, and returns what the read met that
/// the cycle carries on past.
fn write_snapshot(
    service: &SensorThingsService,
    cursor: Option<CursorCycle>,
    cycle_start: CycleStart,
    downloader: Downloader,
    shutdown: &'static AtomicBool,
    path: &Path,
) -> Result<CollectionWarnings, CollectorError> {
    let settings = service.settings();
    // The header and the reader each name the root, while the service keeps its own.
    let header = SnapshotHeader::new(service.root().clone(), settings.read().kind(), cycle_start.instant());
    let mut reader = PageReader::new(service.root().clone(), downloader, settings.delay(), shutdown);
    let file = File::create(path).map_err(unwritable(path))?;
    let snapshot = SnapshotWriter::start(BufWriter::new(file), &header).map_err(unwritable(path))?;
    match (settings.read(), cursor) {
        (ReadMode::Latest, _) => read_latest(&mut reader, service, snapshot, path),
        (ReadMode::Range(range), _) => read_range(&mut reader, service, *range, cycle_start, snapshot, path),
        (ReadMode::Incremental(incremental), Some(cursor)) => read_incremental(&mut reader, service, incremental, cursor, cycle_start, snapshot, path),
        (ReadMode::Incremental(_), None) => Err(CollectorError::CursorWithheld {
            // The error names the root, while the service keeps its own.
            root: Box::new(service.root().clone()),
        }),
    }
}

/// Reads the newest reading of every data stream `service` selects into `snapshot`, after its
/// header, and returns the collection limit that stopped the read, if one did.
fn read_latest<W: Write>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    mut snapshot: SnapshotWriter<W>,
    path: &Path,
) -> Result<CollectionWarnings, CollectorError> {
    let settings = service.settings();
    let end = traverse(
        reader,
        latest_discovery_request(service.root(), settings),
        settings.limits().pages(),
        |datastream| {
            snapshot.datastream(&DatastreamLine::new(datastream)).map_err(unwritable(path))?;
            Ok::<_, CollectorError>(ControlFlow::Continue(()))
        },
    )?;
    snapshot.finish().map_err(unwritable(path))?;
    let mut warnings = CollectionWarnings::default();
    if let TraversalEnd::LimitReached(limit) = end {
        warnings.limit_reached(limit);
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use crate::{
        collection_warnings::CollectionWarnings,
        error::CollectorError,
        sensorthings::{
            limit_reached::{CollectLimit, LimitReached},
            test_collection::{RUNNING, collect, collector, cycle_start, root, snapshot_lines},
        },
    };
    use cassiopeia_common::{
        format::DataFormat,
        input::Input,
        request_header::{RequestHeader, RequestHeaders},
    };
    use cassiopeia_ir::payload::FilePayload;
    use cassiopeia_sensorthings::{
        paging::next_link::NextLinkError,
        read_kind::ReadKind,
        settings::collect_limits::PageLimit,
        snapshot::header::SnapshotHeader,
    };
    use mockito::{Matcher, Mock, ServerGuard};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{
        fs,
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };
    use url::Url;

    /// The path and query of the latest discovery request with the default settings.
    const LATEST: &str =
        "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000";

    /// A shutdown flag the stop test sets once the first page is served.
    static STOPPED_AFTER_PAGE_ONE: AtomicBool = AtomicBool::new(false);

    /// The ids of the `datastream` lines of a snapshot, in file order.
    fn datastream_ids(lines: &[Value]) -> Vec<Value> {
        lines.iter().skip(1).map(|line| line["datastream"]["@iot.id"].clone()).collect()
    }

    /// A page body holding Datastreams `ids` and, when given, the next link `next`.
    fn page(ids: &[u64], next: Option<&str>) -> String {
        let mut body = json!({"value": ids.iter().map(|id| json!({"@iot.id": id, "name": format!("stream {id}")})).collect::<Vec<_>>()});
        if let Some(next) = next {
            body["@iot.nextLink"] = json!(next);
        }
        body.to_string()
    }

    /// Serves `body` at `path_and_query`, once.
    fn serve(server: &mut ServerGuard, path_and_query: &str, body: String) -> Mock {
        server.mock("GET", path_and_query).with_body(body).expect(1).create()
    }

    /// Serves three pages joined by absolute next links: Datastreams 1, 2 and 3.
    fn three_pages(server: &mut ServerGuard) -> [Mock; 3] {
        let url = server.url();
        [
            serve(server, LATEST, page(&[1], Some(&format!("{url}/v1.1/Datastreams?$skip=1")))),
            serve(
                server,
                "/v1.1/Datastreams?$skip=1",
                page(&[2], Some(&format!("{url}/v1.1/Datastreams?$skip=2"))),
            ),
            serve(server, "/v1.1/Datastreams?$skip=2", page(&[3], None)),
        ]
    }

    #[test]
    fn latest_read_writes_the_header_and_every_data_stream() {
        let mut server = mockito::Server::new();
        let discovery = serve(&mut server, LATEST, page(&[56775, 56776], None));

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        assert_eq!(result.unwrap(), CollectionWarnings::default());
        discovery.assert();
        let lines = snapshot_lines(payloads);
        assert_eq!(lines.len(), 3);
        assert_eq!(
            SnapshotHeader::from_line(&lines[0].to_string()).unwrap(),
            SnapshotHeader::new(root(&server), ReadKind::Latest, cycle_start().instant())
        );
        assert_eq!(lines[1], json!({"datastream": {"@iot.id": 56775, "name": "stream 56775"}}));
        assert_eq!(datastream_ids(&lines), [json!(56775), json!(56776)]);
    }

    #[test]
    fn three_pages_are_followed_in_order() {
        let mut server = mockito::Server::new();
        let pages = three_pages(&mut server);

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        result.unwrap();
        pages.iter().for_each(Mock::assert);
        assert_eq!(datastream_ids(&snapshot_lines(payloads)), [json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn a_relative_next_link_is_followed() {
        let mut server = mockito::Server::new();
        let first = serve(&mut server, LATEST, page(&[1], Some("Datastreams?$skip=1000")));
        let second = serve(&mut server, "/v1.1/Datastreams?$skip=1000", page(&[2], None));

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        result.unwrap();
        first.assert();
        second.assert();
        assert_eq!(datastream_ids(&snapshot_lines(payloads)), [json!(1), json!(2)]);
    }

    #[test]
    fn a_foreign_next_link_fails_without_contacting_its_server() {
        let mut server = mockito::Server::new();
        let mut other = mockito::Server::new();
        let foreign = format!("{}/v1.1/Datastreams?$skip=1", other.url());
        let untouched = other.mock("GET", Matcher::Any).expect(0).create();
        serve(&mut server, LATEST, page(&[1], Some(&foreign)));

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        let error = result.unwrap_err();
        assert!(
            matches!(&error, CollectorError::NextLink { source: NextLinkError::LeavesService { link, .. }, .. } if link.as_str() == foreign),
            "{error:?}"
        );
        untouched.assert();
        assert_eq!(payloads.len(), 0);
    }

    #[test]
    fn a_page_linking_to_itself_fails_as_a_loop() {
        let mut server = mockito::Server::new();
        let itself = format!("{}{LATEST}", server.url());
        let discovery = serve(&mut server, LATEST, page(&[1], Some(&itself)));

        let (result, _) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        discovery.assert();
        assert!(matches!(result, Err(CollectorError::LinkLoop { link }) if link.as_str() == itself));
    }

    #[test]
    fn a_transient_failure_on_a_middle_page_is_retried() {
        let mut server = mockito::Server::new();
        let url = server.url();
        serve(&mut server, LATEST, page(&[1], Some(&format!("{url}/v1.1/Datastreams?$skip=1"))));
        let busy = server.mock("GET", "/v1.1/Datastreams?$skip=1").with_status(503).expect(1).create();
        let delivered = serve(&mut server, "/v1.1/Datastreams?$skip=1", page(&[2], None));

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        result.unwrap();
        busy.assert();
        delivered.assert();
        assert_eq!(datastream_ids(&snapshot_lines(payloads)), [json!(1), json!(2)]);
    }

    #[test]
    fn a_permanent_failure_names_its_page() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Datastreams?$skip=1", server.url());
        serve(&mut server, LATEST, page(&[1], Some(&second)));
        let failing = server.mock("GET", "/v1.1/Datastreams?$skip=1").with_status(500).expect(3).create();

        let (result, _) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        failing.assert();
        let error = result.unwrap_err();
        assert!(
            matches!(&error, CollectorError::HttpStatus { status, .. } if status.as_u16() == 500),
            "{error:?}"
        );
        assert!(error.to_string().contains(&second), "{error}");
    }

    #[test]
    fn the_page_limit_stops_collection_with_a_warning() {
        let mut server = mockito::Server::new();
        let url = server.url();
        serve(&mut server, LATEST, page(&[1], Some(&format!("{url}/v1.1/Datastreams?$skip=1"))));
        serve(
            &mut server,
            "/v1.1/Datastreams?$skip=1",
            page(&[2], Some(&format!("{url}/v1.1/Datastreams?$skip=2"))),
        );
        let third = server.mock("GET", "/v1.1/Datastreams?$skip=2").expect(0).create();

        let (result, payloads) = collect(collector(&server, r#"{"limits": {"pages": 2}}"#, RequestHeaders::default(), &RUNNING));

        third.assert();
        assert_eq!(
            result.unwrap().limits_reached(),
            [LimitReached::new(
                root(&server),
                CollectLimit::Pages(PageLimit::try_from(2).unwrap()),
                Url::parse(&format!("{url}/v1.1/Datastreams?$skip=2")).unwrap(),
            )]
        );
        assert_eq!(datastream_ids(&snapshot_lines(payloads)), [json!(1), json!(2)]);
    }

    #[test]
    fn configured_headers_go_with_every_page() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let pages = [
            (LATEST.to_owned(), page(&[1], Some(&format!("{url}/v1.1/Datastreams?$skip=1")))),
            (
                "/v1.1/Datastreams?$skip=1".to_owned(),
                page(&[2], Some(&format!("{url}/v1.1/Datastreams?$skip=2"))),
            ),
            ("/v1.1/Datastreams?$skip=2".to_owned(), page(&[3], None)),
        ]
        .map(|(path, body)| {
            server
                .mock("GET", path.as_str())
                .match_header("authorization", "Bearer t")
                .with_body(body)
                .expect(1)
                .create()
        });
        let headers = RequestHeaders::new(vec![RequestHeader::new("Authorization", "Bearer t").unwrap()]);

        let (result, payloads) = collect(collector(&server, "{}", headers, &RUNNING));

        result.unwrap();
        pages.iter().for_each(Mock::assert);
        assert_eq!(datastream_ids(&snapshot_lines(payloads)), [json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn a_page_that_is_no_collection_fails_the_lane() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, "<html>".to_owned());

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        let error = result.unwrap_err();
        assert!(
            matches!(&error, CollectorError::PageUnreadable { url, .. } if url.as_str().ends_with(LATEST)),
            "{error:?}"
        );
        assert!(error.to_string().contains("answered with no page of a collection"), "{error}");
        assert_eq!(payloads.len(), 0);
    }

    #[test]
    fn the_delay_spaces_requests() {
        let mut server = mockito::Server::new();
        three_pages(&mut server);
        let started = Instant::now();

        let (result, payloads) = collect(collector(&server, r#"{"delay": "50ms"}"#, RequestHeaders::default(), &RUNNING));

        result.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(100), "took {:?}", started.elapsed());
        assert_eq!(datastream_ids(&snapshot_lines(payloads)).len(), 3);
    }

    #[test]
    fn a_stop_between_pages_interrupts_collection() {
        let mut server = mockito::Server::new();
        let body = page(&[1], Some(&format!("{}/v1.1/Datastreams?$skip=1", server.url())));
        server
            .mock("GET", LATEST)
            .with_body_from_request(move |_| {
                STOPPED_AFTER_PAGE_ONE.store(true, Ordering::Release);
                body.clone().into_bytes()
            })
            .expect(1)
            .create();
        let second = server.mock("GET", "/v1.1/Datastreams?$skip=1").expect(0).create();

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &STOPPED_AFTER_PAGE_ONE));

        second.assert();
        assert!(matches!(result, Err(CollectorError::Interrupted { .. })), "{result:?}");
        assert_eq!(payloads.len(), 0);
    }

    #[test]
    fn the_snapshot_is_sent_as_a_sensorthings_file() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, page(&[1], None));

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        result.unwrap();
        let [payload] = <[FilePayload; 1]>::try_from(payloads).unwrap();
        assert_eq!(payload.format_hint(), &Some(DataFormat::SensorThings));
        let (path, source) = payload.into_path_and_source();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(path.extension().and_then(|extension| extension.to_str()), Some("jsonl"));
        assert_eq!(source.to_string(), format!("{}/v1.1", server.url()));
        assert!(matches!(source, Input::Remote(_)));
    }
}

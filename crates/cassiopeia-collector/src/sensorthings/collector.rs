use crate::{
    collection_warnings::CollectionWarnings,
    collector::Collector,
    downloader::Downloader,
    error::CollectorError,
    sensorthings::{
        cursor_cycle::CursorCycle,
        incremental_read::read_incremental,
        multi_datastreams_unread::MultiDatastreamsUnread,
        page_reader::PageReader,
        range_read::read_range,
        snapshot_failure::unwritable,
        stream_discovery::discover_streams,
        traversal::{TraversalEnd, traverse},
        unread_check::UnreadCheck,
    },
};
use cassiopeia_common::{channel::ChannelSender, cycle_start::CycleStart, format::DataFormat, masked_url::MaskedUrl, signal::Signal};
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ir::payload::{CollectedPayload, FilePayload};
use cassiopeia_sensorthings::{
    request::discovery_request::{latest_discovery_request, things_discovery_request},
    service::SensorThingsService,
    settings::{multi_datastreams::MultiDatastreams, read_mode::ReadMode},
    snapshot::{
        header::SnapshotHeader,
        line::{DatastreamLine, ThingLine},
        writer::{SnapshotWriter, ThingWriter},
    },
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
    /// Whether the cycle checks the server for `MultiDatastreams` the input reads none of.
    unread_check: UnreadCheck,
    /// The instant the cycle started, which the snapshot records as its collection time.
    cycle_start: CycleStart,
    /// The downloader every page is fetched through, with the input's headers.
    downloader: Downloader,
    /// The process-wide shutdown flag, set while the run is being stopped.
    shutdown: &'static AtomicBool,
}

impl SensorThingsCollector {
    /// A collector of `service` for the cycle that started at `cycle_start`, resuming an incremental
    /// read from `cursor`, checking for unread `MultiDatastreams` when `unread_check` is due, fetching
    /// through `downloader` and stopping once `shutdown` is set.
    #[must_use]
    pub const fn new(
        service: SensorThingsService,
        cursor: Option<CursorCycle>,
        unread_check: UnreadCheck,
        cycle_start: CycleStart,
        downloader: Downloader,
        shutdown: &'static AtomicBool,
    ) -> SensorThingsCollector {
        SensorThingsCollector {
            service,
            cursor,
            unread_check,
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
            unread_check,
            cycle_start,
            downloader,
            shutdown,
        } = *self;
        let directory = TempDir::new().map_err(|source| CollectorError::Io { source, path: env::temp_dir() })?;
        // The snapshot must outlive the collector, because the profiler and the ingestor read it
        // later; cleanup is the caller's responsibility, as for every downloaded source.
        let directory = directory.dont_delete_on_drop();
        let path = directory.path().join(format!("snapshot.{}", DataFormat::SensorThings.extension()));

        let warnings = match write_snapshot(&service, cursor, unread_check, cycle_start, downloader, shutdown, &path) {
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
///
/// Once the read has succeeded, a cycle whose `unread_check` is due checks whether the server holds
/// `MultiDatastreams` an input that reads none leaves unread.
fn write_snapshot(
    service: &SensorThingsService,
    cursor: Option<CursorCycle>,
    unread_check: UnreadCheck,
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
    let mut warnings = match (settings.read(), cursor) {
        (ReadMode::Latest, _) => read_latest(&mut reader, service, snapshot, path),
        (ReadMode::Range(range), _) => read_range(&mut reader, service, *range, cycle_start, snapshot, path),
        (ReadMode::Incremental(incremental), Some(cursor)) => read_incremental(&mut reader, service, incremental, cursor, cycle_start, snapshot, path),
        (ReadMode::Incremental(_), None) => Err(CollectorError::CursorWithheld {
            // The error names the root, while the service keeps its own.
            root: Box::new(service.root().clone()),
        }),
    }?;
    if let (UnreadCheck::Due, MultiDatastreams::None) = (unread_check, settings.multi_datastreams())
        && let Some(unread) = MultiDatastreamsUnread::check(&mut reader)
    {
        warnings.multi_datastreams_unread(unread);
    }
    Ok(warnings)
}

/// Reads the newest reading of every data stream `service` selects, its Datastreams and then its
/// `MultiDatastreams`, into `snapshot`, after its header, then the Things it lists by themselves,
/// and returns the collection limits that stopped a discovery, if any did.
fn read_latest<W: Write>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    mut snapshot: SnapshotWriter<W>,
    path: &Path,
) -> Result<CollectionWarnings, CollectorError> {
    let ends = discover_streams(reader, service, latest_discovery_request, |kind, served| {
        snapshot.datastream(&DatastreamLine::new(kind, served)).map_err(unwritable(path))
    })?;
    let mut warnings = CollectionWarnings::default();
    for (_, end) in ends {
        if let TraversalEnd::LimitReached(limit) = end {
            warnings.limit_reached(limit);
        }
    }
    let mut things = snapshot.into_things();
    read_things(reader, service, &mut things, path, &mut warnings)?;
    things.finish().map_err(unwritable(path))?;
    Ok(warnings)
}

/// Writes every Thing `service`'s `things` setting lists into `things`, each with its Locations, in
/// the order the server sent them, and records a page limit that stops the read in `warnings`.
///
/// The Things discovery comes after the data stream discoveries, so a snapshot keeps one order: its
/// header, its data streams, then its Things. It is followed under the paging rules, the pacing and
/// `limits.pages` of every request of the read. A setting that lists no Thing sends nothing.
fn read_things<W: Write>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    things: &mut ThingWriter<W>,
    path: &Path,
    warnings: &mut CollectionWarnings,
) -> Result<(), CollectorError> {
    let Some(request) = things_discovery_request(service.root(), service.settings()) else {
        return Ok(());
    };
    let end = traverse(reader, request, service.settings().limits().pages(), |served| {
        things
            .thing(&ThingLine::new(served))
            .map(|()| ControlFlow::Continue(()))
            .map_err(unwritable(path))
    })?;
    if let TraversalEnd::LimitReached(limit) = end {
        warnings.limit_reached(limit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        collection_warnings::CollectionWarnings,
        error::CollectorError,
        sensorthings::{
            limit_reached::{CollectLimit, LimitReached},
            multi_datastreams_unread::MultiDatastreamsUnread,
            test_collection::{RUNNING, checking_collector, collect, collector, cycle_start, root, snapshot_lines},
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
        "/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc";

    /// The path and query of the latest discovery request that requests features of interest.
    const LATEST_WITH_FEATURES: &str = "/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc";

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
    fn a_latest_read_of_features_of_interest_keeps_each_inside_its_newest_observation() {
        let mut server = mockito::Server::new();
        let datastream = json!({
            "@iot.id": 7,
            "name": "bus-12-pm10",
            "Observations": [{
                "@iot.id": 70,
                "phenomenonTime": "2026-11-12T09:59:00Z",
                "result": 18.2,
                "FeatureOfInterest": {
                    "@iot.id": 4711,
                    "encodingType": "application/geo+json",
                    "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]},
                },
            }],
        });
        let discovery = serve(&mut server, LATEST_WITH_FEATURES, json!({ "value": [&datastream] }).to_string());

        let (result, payloads) = collect(collector(&server, r#"{"featuresOfInterest": true}"#, RequestHeaders::default(), &RUNNING));

        result.unwrap();
        discovery.assert();
        assert_eq!(snapshot_lines(payloads)[1], json!({ "datastream": datastream }));
    }

    #[test]
    fn a_latest_read_keeps_each_sensor_inside_its_data_stream() {
        let mut server = mockito::Server::new();
        let datastream = json!({
            "@iot.id": 56776,
            "name": "station-1 pm10",
            "Sensor": {
                "@iot.id": 7,
                "name": "PM-200",
                "encodingType": "application/pdf",
                "metadata": "https://sensors.example.org/pm-200.pdf",
            },
            "Observations": [{"@iot.id": 70, "phenomenonTime": "2026-11-12T09:59:00Z", "result": 18.2}],
        });
        let discovery = serve(&mut server, LATEST, json!({ "value": [&datastream] }).to_string());

        let (result, payloads) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        result.unwrap();
        discovery.assert();
        assert_eq!(snapshot_lines(payloads)[1], json!({ "datastream": datastream }));
    }

    #[test]
    fn a_latest_read_that_omits_features_of_interest_sends_the_plain_discovery() {
        let mut server = mockito::Server::new();
        let discovery = serve(&mut server, LATEST, page(&[7], None));

        let (result, _) = collect(collector(&server, r#"{"featuresOfInterest": false}"#, RequestHeaders::default(), &RUNNING));

        result.unwrap();
        discovery.assert();
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

    /// The path and query of the request that lists every Thing.
    const THINGS: &str = "/v1.1/Things?$top=1000&$expand=Locations&$orderby=id%20asc";

    /// A Thing as a server lists it, with one `GeoJSON` Location.
    fn thing(id: u64, name: &str) -> Value {
        json!({
            "@iot.id": id,
            "name": name,
            "Locations": [{"@iot.id": id * 10, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [5.61, 51.32]}}],
        })
    }

    #[test]
    fn a_latest_read_that_lists_things_writes_them_after_the_data_streams() {
        let mut server = mockito::Server::new();
        let mocks = [
            serve(&mut server, LATEST, page(&[7], None)),
            serve(
                &mut server,
                THINGS,
                json!({"value": [thing(1, "Stationsplein"), thing(2, "Kennedylaan")]}).to_string(),
            ),
        ];

        let (result, payloads) = collect(collector(&server, r#"{"things": true}"#, RequestHeaders::default(), &RUNNING));

        assert_eq!(result.unwrap(), CollectionWarnings::default());
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(
            lines[1..],
            [
                json!({"datastream": {"@iot.id": 7, "name": "stream 7"}}),
                json!({"thing": thing(1, "Stationsplein")}),
                json!({"thing": thing(2, "Kennedylaan")}),
            ]
        );
    }

    #[test]
    fn a_things_filter_selects_the_things_listed() {
        let mut server = mockito::Server::new();
        let mocks = [
            serve(&mut server, LATEST, page(&[], None)),
            serve(
                &mut server,
                "/v1.1/Things?$filter=startswith(name,%27ILM_%27)&$top=1000&$expand=Locations&$orderby=id%20asc",
                json!({"value": [thing(12520, "ILM_I75")]}).to_string(),
            ),
        ];

        let (result, payloads) = collect(collector(
            &server,
            r#"{"things": "startswith(name,'ILM_')"}"#,
            RequestHeaders::default(),
            &RUNNING,
        ));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(snapshot_lines(payloads)[1..], [json!({"thing": thing(12520, "ILM_I75")})]);
    }

    #[test]
    fn a_latest_read_without_things_lists_none() {
        for settings in ["{}", r#"{"things": false}"#] {
            let mut server = mockito::Server::new();
            let discovery = serve(&mut server, LATEST, page(&[7], None));
            let untouched = server.mock("GET", Matcher::Regex("^/v1.1/Things".to_owned())).expect(0).create();

            let (result, payloads) = collect(collector(&server, settings, RequestHeaders::default(), &RUNNING));

            result.unwrap();
            discovery.assert();
            untouched.assert();
            assert_eq!(snapshot_lines(payloads).len(), 2, "{settings}");
        }
    }

    #[test]
    fn the_page_limit_stops_the_things_listing_with_a_warning() {
        let mut server = mockito::Server::new();
        let next = format!("{}/v1.1/Things?$skip=2", server.url());
        let mocks = [
            serve(
                &mut server,
                "/v1.1/Datastreams?$top=2&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc",
                page(&[], None),
            ),
            serve(
                &mut server,
                "/v1.1/Things?$top=2&$expand=Locations&$orderby=id%20asc",
                json!({"value": [thing(1, "Stationsplein"), thing(2, "Kennedylaan")], "@iot.nextLink": next}).to_string(),
            ),
        ];
        let untouched = server.mock("GET", "/v1.1/Things?$skip=2").expect(0).create();

        let (result, payloads) = collect(collector(
            &server,
            r#"{"things": true, "pageSize": 2, "limits": {"pages": 1}}"#,
            RequestHeaders::default(),
            &RUNNING,
        ));

        mocks.iter().for_each(Mock::assert);
        untouched.assert();
        assert_eq!(
            result.unwrap().limits_reached(),
            [LimitReached::new(
                root(&server),
                CollectLimit::Pages(PageLimit::try_from(1).unwrap()),
                Url::parse(&next).unwrap(),
            )]
        );
        assert_eq!(
            snapshot_lines(payloads)[1..],
            [json!({"thing": thing(1, "Stationsplein")}), json!({"thing": thing(2, "Kennedylaan")})]
        );
    }

    #[test]
    fn a_things_listing_follows_its_next_links_after_every_data_stream() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let mocks = [
            serve(&mut server, LATEST, page(&[7], None)),
            serve(&mut server, MULTI_LATEST, page(&[4], None)),
            serve(
                &mut server,
                THINGS,
                json!({"value": [thing(1, "Stationsplein")], "@iot.nextLink": format!("{url}/v1.1/Things?$skip=1")}).to_string(),
            ),
            serve(&mut server, "/v1.1/Things?$skip=1", json!({"value": [thing(2, "Kennedylaan")]}).to_string()),
        ];

        let (result, payloads) = collect(collector(
            &server,
            r#"{"things": true, "multiDatastreams": true}"#,
            RequestHeaders::default(),
            &RUNNING,
        ));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            snapshot_lines(payloads)[1..],
            [
                json!({"datastream": {"@iot.id": 7, "name": "stream 7"}}),
                json!({"multiDatastream": {"@iot.id": 4, "name": "stream 4"}}),
                json!({"thing": thing(1, "Stationsplein")}),
                json!({"thing": thing(2, "Kennedylaan")}),
            ]
        );
    }

    /// The path and query of the latest discovery of every `MultiDatastream`.
    const MULTI_LATEST: &str = "/v1.1/MultiDatastreams?$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc";

    /// The path and query of the request for at most one `MultiDatastream`.
    const ANY_MULTI_DATASTREAM: &str = "/v1.1/MultiDatastreams?$top=1";

    /// A service root document that lists the collections `names`.
    fn service_document(names: &[&str]) -> String {
        json!({"value": names.iter().map(|name| json!({"name": name, "url": format!("https://h.example/v1.1/{name}")})).collect::<Vec<_>>()}).to_string()
    }

    #[test]
    fn a_latest_read_of_multi_datastreams_writes_them_after_the_datastreams() {
        let mut server = mockito::Server::new();
        let wind = json!({
            "@iot.id": 4,
            "name": "ws-7",
            "ObservedProperties": [{"@iot.id": 11, "name": "windSpeed"}, {"@iot.id": 12, "name": "windDirection"}],
            "Observations": [{"@iot.id": 501, "phenomenonTime": "2026-11-12T09:50:00Z", "result": [4.2, 270]}],
        });
        let mocks = [
            serve(&mut server, LATEST, page(&[1], None)),
            serve(&mut server, MULTI_LATEST, json!({ "value": [&wind] }).to_string()),
        ];

        let (result, payloads) = collect(collector(&server, r#"{"multiDatastreams": true}"#, RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(
            lines[1..],
            [json!({"datastream": {"@iot.id": 1, "name": "stream 1"}}), json!({ "multiDatastream": wind })]
        );
    }

    #[test]
    fn a_read_of_no_multi_datastream_reports_those_its_server_holds_once_checked() {
        let mut server = mockito::Server::new();
        let mocks = [
            serve(&mut server, LATEST, page(&[1], None)),
            serve(&mut server, "/v1.1", service_document(&["Datastreams", "MultiDatastreams"])),
            serve(&mut server, ANY_MULTI_DATASTREAM, json!({"value": [{"@iot.id": 4}]}).to_string()),
        ];
        let untouched = server.mock("GET", MULTI_LATEST).expect(0).create();

        let (result, payloads) = collect(checking_collector(&server, "{}"));

        mocks.iter().for_each(Mock::assert);
        untouched.assert();
        assert_eq!(result.unwrap().unread_multi_datastreams(), [MultiDatastreamsUnread::new(root(&server))]);
        assert_eq!(snapshot_lines(payloads).len(), 2);
    }

    #[test]
    fn a_server_whose_root_lists_no_multi_datastreams_is_asked_for_none() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, page(&[1], None));
        let root_document = serve(&mut server, "/v1.1", service_document(&["Datastreams"]));
        let untouched = server.mock("GET", ANY_MULTI_DATASTREAM).expect(0).create();

        let (result, _) = collect(checking_collector(&server, "{}"));

        root_document.assert();
        untouched.assert();
        assert_eq!(result.unwrap(), CollectionWarnings::default());
    }

    #[test]
    fn a_server_without_multi_datastreams_is_not_reported() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, page(&[1], None));
        serve(&mut server, "/v1.1", service_document(&["Datastreams", "MultiDatastreams"]));
        let probe = serve(&mut server, ANY_MULTI_DATASTREAM, json!({"value": []}).to_string());

        let (result, _) = collect(checking_collector(&server, "{}"));

        probe.assert();
        assert_eq!(result.unwrap(), CollectionWarnings::default());
    }

    #[test]
    fn a_failed_check_reports_nothing_and_keeps_the_read() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, page(&[1], None));
        server.mock("GET", "/v1.1").with_status(404).create();

        let (result, payloads) = collect(checking_collector(&server, "{}"));

        assert_eq!(result.unwrap(), CollectionWarnings::default());
        assert_eq!(snapshot_lines(payloads).len(), 2);
    }

    #[test]
    fn a_read_of_multi_datastreams_never_checks() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, page(&[1], None));
        serve(&mut server, MULTI_LATEST, page(&[], None));
        let untouched = server.mock("GET", "/v1.1").expect(0).create();

        let (result, _) = collect(checking_collector(&server, r#"{"multiDatastreams": true}"#));

        untouched.assert();
        assert_eq!(result.unwrap(), CollectionWarnings::default());
    }

    #[test]
    fn a_cycle_after_the_check_sends_no_check() {
        let mut server = mockito::Server::new();
        serve(&mut server, LATEST, page(&[1], None));
        let untouched = server.mock("GET", "/v1.1").expect(0).create();

        let (result, _) = collect(collector(&server, "{}", RequestHeaders::default(), &RUNNING));

        untouched.assert();
        assert_eq!(result.unwrap(), CollectionWarnings::default());
    }
}

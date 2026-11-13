use crate::{
    collection_warnings::CollectionWarnings,
    error::CollectorError,
    sensorthings::{
        observation_read::{ObservationSink, next_stream, read_observations},
        page_reader::PageReader,
        range_empty::RangeEmpty,
        snapshot_failure::unwritable,
        traversal::{TraversalEnd, traverse},
    },
};
use cassiopeia_common::cycle_start::CycleStart;
use cassiopeia_sensorthings::{
    range_datastreams::RangeDatastreams,
    request::{discovery_request::metadata_discovery_request, observations_request::observations_request},
    service::SensorThingsService,
    settings::time_range::TimeRange,
    snapshot::{line::DatastreamLine, writer::SnapshotWriter},
};
use cassiopeia_sensorthings_cursor::seen_observations::SeenObservations;
use std::{io::Write, ops::ControlFlow, path::Path};

/// Reads every reading of `range` in the cycle that started at `cycle_start` from the data streams
/// `service` selects into `snapshot`, after its header, and returns what the read met that the cycle
/// carries on past.
///
/// When the range holds no reading this cycle, nothing is requested and the snapshot is its header
/// alone. Otherwise the metadata discovery comes first, and every Datastream it serves is written as
/// it was served. Then each data stream that may hold a reading of the window is read on its own, by
/// ascending id, oldest reading first. A page limit stops one data stream's read and the read goes on
/// with the next; the observation limit stops the whole read.
///
/// # Errors
///
/// Returns every [`CollectorError`] a traversal fails with, and [`CollectorError::Io`] when the
/// snapshot at `path` cannot be written.
pub(crate) fn read_range<W: Write>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    range: TimeRange,
    cycle_start: CycleStart,
    mut snapshot: SnapshotWriter<W>,
    path: &Path,
) -> Result<CollectionWarnings, CollectorError> {
    let mut warnings = CollectionWarnings::default();
    let window = match range.window(cycle_start.instant()) {
        Ok(window) => window,
        Err(empty) => {
            snapshot.finish().map_err(unwritable(path))?;
            // The warning names the root, while the service keeps its own.
            warnings.range_empty(RangeEmpty::new(service.root().clone(), empty));
            return Ok(warnings);
        }
    };
    let settings = service.settings();
    let mut datastreams = RangeDatastreams::default();
    let discovery = traverse(
        reader,
        metadata_discovery_request(service.root(), settings),
        settings.limits().pages(),
        |datastream| {
            datastreams.add(&datastream, window);
            snapshot.datastream(&DatastreamLine::new(datastream)).map_err(unwritable(path))?;
            Ok::<_, CollectorError>(ControlFlow::Continue(()))
        },
    )?;
    if let TraversalEnd::LimitReached(limit) = discovery {
        warnings.limit_reached(limit);
    }
    let mut sink = ObservationSink::new(snapshot.into_observations(), settings.limits().observations(), path);
    // A range read keeps nothing between cycles, so no Observation is seen before it is read.
    let unseen = SeenObservations::default();
    for datastream in datastreams.into_ids() {
        let request = observations_request(service.root(), &datastream, window, settings.page_size());
        let end = read_observations(reader, request, settings.limits().pages(), &datastream, &unseen, &mut sink, |_, _| {})?;
        if next_stream(end, &mut warnings).is_break() {
            break;
        }
    }
    sink.finish()?;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        limit_reached::{CollectLimit, LimitReached},
        range_empty::RangeEmpty,
        test_collection::{RUNNING, collect, collector, root, snapshot_lines},
    };
    use cassiopeia_common::request_header::RequestHeaders;
    use cassiopeia_sensorthings::{
        read_kind::ReadKind,
        read_window::ReadWindow,
        settings::collect_limits::{ObservationLimit, PageLimit},
        snapshot::header::SnapshotHeader,
    };
    use chrono::{DateTime, Utc};
    use mockito::{Matcher, Mock, ServerGuard};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::slice;
    use url::Url;

    /// The path and query of the metadata discovery request with the default settings.
    const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=1000";

    /// The bounds of a `last: "24h"` range in the cycle the tests collect in.
    const LAST_DAY: (&str, &str) = ("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z");

    /// The settings of a range read of `range`, a JSON object, with the members `extra` beside it.
    fn range_read(range: &str, extra: &str) -> String {
        format!(r#"{{"read": {{"range": {range}}}{extra}}}"#)
    }

    /// The path and query of the observations request of the data stream `id` between `bounds`.
    fn observations_path(id: u64, bounds: (&str, &str)) -> String {
        format!(
            "/v1.1/Datastreams({id})/Observations?$filter=phenomenonTime%20ge%20{}%20and%20phenomenonTime%20lt%20{}&$orderby=phenomenonTime%20asc,id%20asc&$top=1000",
            bounds.0, bounds.1
        )
    }

    /// A page body holding `items` and, when given, the next link `next`.
    fn page(items: &[Value], next: Option<&str>) -> String {
        let mut body = json!({ "value": items });
        if let Some(next) = next {
            body["@iot.nextLink"] = json!(next);
        }
        body.to_string()
    }

    /// Served Observations `ids`, each made at an hour of 2026-11-12 as late as its id.
    fn observations(ids: &[u64]) -> Vec<Value> {
        ids.iter()
            .map(|id| json!({"@iot.id": id, "phenomenonTime": format!("2026-11-12T{:02}:00:00Z", id % 10), "result": id}))
            .collect()
    }

    /// Serves `body` at `path_and_query`, once.
    fn serve(server: &mut ServerGuard, path_and_query: &str, body: String) -> Mock {
        server.mock("GET", path_and_query).with_body(body).expect(1).create()
    }

    /// Serves the metadata discovery with Datastreams `ids`, each without a `phenomenonTime`.
    fn discovery(server: &mut ServerGuard, ids: &[u64]) -> Mock {
        let datastreams: Vec<Value> = ids.iter().map(|id| json!({"@iot.id": id, "name": format!("stream {id}")})).collect();
        serve(server, METADATA, page(&datastreams, None))
    }

    /// Each `observation` line of a snapshot as the id of its Observation and of its data stream, in
    /// file order.
    fn observation_lines(lines: &[Value]) -> Vec<(Value, Value)> {
        lines
            .iter()
            .filter(|line| line.get("observation").is_some())
            .map(|line| (line["observation"]["@iot.id"].clone(), line["datastream"].clone()))
            .collect()
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    #[test]
    fn a_last_range_bounds_every_request_at_the_cycle_start() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[1, 2]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11]), None)),
            serve(&mut server, &observations_path(2, LAST_DAY), page(&observations(&[21]), None)),
        ];

        let (result, payloads) = collect(collector(&server, &range_read(r#"{"last": "24h"}"#, ""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(SnapshotHeader::from_line(&lines[0].to_string()).unwrap().read(), ReadKind::Range);
        assert_eq!(lines[1], json!({"datastream": {"@iot.id": 1, "name": "stream 1"}}));
        assert_eq!(observation_lines(&lines), [(json!(11), json!(1)), (json!(21), json!(2))]);
        assert_eq!(
            lines[3],
            json!({"observation": {"@iot.id": 11, "phenomenonTime": "2026-11-12T01:00:00Z", "result": 11}, "datastream": 1})
        );
    }

    #[test]
    fn an_absolute_range_is_used_as_written() {
        let mut server = mockito::Server::new();
        let bounds = ("2026-11-01T00:00:00Z", "2026-11-02T00:00:00Z");
        let mocks = [
            discovery(&mut server, &[1]),
            serve(&mut server, &observations_path(1, bounds), page(&observations(&[11]), None)),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"from": "2026-11-01T00:00:00Z", "to": "2026-11-02T00:00:00Z"}"#, ""),
            RequestHeaders::default(),
            &RUNNING,
        ));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(observation_lines(&snapshot_lines(payloads)), [(json!(11), json!(1))]);
    }

    #[test]
    fn an_open_absolute_range_ends_at_the_cycle_start() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[1]),
            serve(
                &mut server,
                &observations_path(1, ("2026-11-01T00:00:00Z", "2026-11-12T10:00:00Z")),
                page(&[], None),
            ),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"from": "2026-11-01T00:00:00Z"}"#, ""),
            RequestHeaders::default(),
            &RUNNING,
        ));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(observation_lines(&snapshot_lines(payloads)), []);
    }

    #[test]
    fn a_stream_that_ended_before_the_range_is_not_requested() {
        let mut server = mockito::Server::new();
        let ended = json!({"@iot.id": 1, "phenomenonTime": "2026-01-01T00:00:00Z/2026-01-02T00:00:00Z"});
        let discovered = serve(&mut server, METADATA, page(slice::from_ref(&ended), None));
        let untouched = server.mock("GET", Matcher::Regex(r"^/v1\.1/Datastreams\(1\)/".to_owned())).expect(0).create();

        let (result, payloads) = collect(collector(&server, &range_read(r#"{"last": "24h"}"#, ""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        discovered.assert();
        untouched.assert();
        assert_eq!(snapshot_lines(payloads)[1..], [json!({ "datastream": ended })]);
    }

    #[test]
    fn a_stream_without_phenomenon_time_is_requested() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[1]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11]), None)),
        ];

        let (result, _) = collect(collector(&server, &range_read(r#"{"last": "24h"}"#, ""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
    }

    #[test]
    fn an_observation_repeated_across_pages_is_written_once() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Datastreams(1)/Observations?$skip=2", server.url());
        let mocks = [
            discovery(&mut server, &[1]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[1, 2]), Some(&second))),
            serve(&mut server, "/v1.1/Datastreams(1)/Observations?$skip=2", page(&observations(&[2, 3]), None)),
        ];

        let (result, payloads) = collect(collector(&server, &range_read(r#"{"last": "24h"}"#, ""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            observation_lines(&snapshot_lines(payloads)),
            [(json!(1), json!(1)), (json!(2), json!(1)), (json!(3), json!(1))]
        );
    }

    #[test]
    fn the_observation_limit_stops_with_a_warning() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[1, 2]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[1, 2, 3, 4]), None)),
            serve(&mut server, &observations_path(2, LAST_DAY), page(&observations(&[5, 6, 7, 8]), None)),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "limits": {"observations": 5}"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            result.unwrap().limits_reached(),
            [LimitReached::new(
                root(&server),
                CollectLimit::Observations(ObservationLimit::try_from(5).unwrap()),
                Url::parse(&format!("{}{}", server.url(), observations_path(2, LAST_DAY))).unwrap(),
            )]
        );
        let written: Vec<Value> = observation_lines(&snapshot_lines(payloads)).into_iter().map(|(id, _)| id).collect();
        assert_eq!(written, [json!(1), json!(2), json!(3), json!(4), json!(5)]);
    }

    #[test]
    fn the_page_limit_ends_each_stream_and_warns_once() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let mocks = [
            discovery(&mut server, &[1, 2]),
            serve(
                &mut server,
                &observations_path(1, LAST_DAY),
                page(&observations(&[1]), Some(&format!("{url}/v1.1/Datastreams(1)/Observations?$skip=1"))),
            ),
            serve(
                &mut server,
                &observations_path(2, LAST_DAY),
                page(&observations(&[2]), Some(&format!("{url}/v1.1/Datastreams(2)/Observations?$skip=1"))),
            ),
        ];
        let unfollowed = server.mock("GET", Matcher::Regex(r"\$skip=1$".to_owned())).expect(0).create();

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "limits": {"pages": 1}"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        mocks.iter().for_each(Mock::assert);
        unfollowed.assert();
        assert_eq!(
            result.unwrap().limits_reached(),
            [LimitReached::new(
                root(&server),
                CollectLimit::Pages(PageLimit::try_from(1).unwrap()),
                Url::parse(&format!("{url}/v1.1/Datastreams(1)/Observations?$skip=1")).unwrap(),
            )]
        );
        assert_eq!(observation_lines(&snapshot_lines(payloads)), [(json!(1), json!(1)), (json!(2), json!(2))]);
    }

    #[test]
    fn a_range_after_the_cycle_start_reads_nothing() {
        let mut server = mockito::Server::new();
        let untouched = server.mock("GET", Matcher::Any).expect(0).create();

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"from": "2026-11-13T00:00:00Z"}"#, ""),
            RequestHeaders::default(),
            &RUNNING,
        ));

        untouched.assert();
        let empty = ReadWindow::new(instant("2026-11-13T00:00:00Z"), instant("2026-11-12T10:00:00Z")).unwrap_err();
        let warnings = result.unwrap();
        assert_eq!(warnings.empty_ranges(), [RangeEmpty::new(root(&server), empty)]);
        assert_eq!(warnings.limits_reached(), []);
        let lines = snapshot_lines(payloads);
        assert_eq!(lines.len(), 1);
        assert_eq!(SnapshotHeader::from_line(&lines[0].to_string()).unwrap().read(), ReadKind::Range);
    }

    #[test]
    fn streams_are_read_in_id_order() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[9, 5, 7]),
            serve(&mut server, &observations_path(5, LAST_DAY), page(&observations(&[51]), None)),
            serve(&mut server, &observations_path(7, LAST_DAY), page(&observations(&[71]), None)),
            serve(&mut server, &observations_path(9, LAST_DAY), page(&observations(&[91]), None)),
        ];

        let (result, payloads) = collect(collector(&server, &range_read(r#"{"last": "24h"}"#, ""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        let discovered: Vec<Value> = lines[1..4].iter().map(|line| line["datastream"]["@iot.id"].clone()).collect();
        assert_eq!(discovered, [json!(9), json!(5), json!(7)]);
        assert_eq!(observation_lines(&lines), [(json!(51), json!(5)), (json!(71), json!(7)), (json!(91), json!(9))]);
    }
}

use crate::{
    collection_warnings::CollectionWarnings,
    error::CollectorError,
    sensorthings::{
        historical_location_read::read_historical_locations,
        observation_read::{ObservationSink, next_stream, read_observations},
        page_reader::PageReader,
        range_empty::RangeEmpty,
        snapshot_failure::unwritable,
        stream_discovery::discover_streams,
        traversal::TraversalEnd,
    },
};
use cassiopeia_common::cycle_start::CycleStart;
use cassiopeia_sensorthings::{
    listed_things::ListedThings,
    range_datastreams::RangeDatastreams,
    request::{discovery_request::metadata_discovery_request, observations_request::observations_request},
    service::SensorThingsService,
    settings::{historical_locations::HistoricalLocations, time_range::TimeRange},
    snapshot::{line::DatastreamLine, writer::SnapshotWriter},
};
use cassiopeia_sensorthings_cursor::seen_entities::SeenEntities;
use std::{io::Write, path::Path};

/// Reads every reading of `range` in the cycle that started at `cycle_start` from the data streams
/// `service` selects into `snapshot`, after its header, and returns what the read met that the cycle
/// carries on past.
///
/// When the range holds no reading this cycle, nothing is requested and the snapshot is its header
/// alone. Otherwise the metadata discoveries come first, of the Datastreams and then of the
/// `MultiDatastreams` the settings select, and every data stream they serve is written as it was
/// served. Then each data stream that may hold a reading of the window is read on its own,
/// Datastreams first, each kind by ascending id, oldest reading first, and each Observation is
/// written as it was served, with its `FeatureOfInterest` when the settings request features of
/// interest; those served without it are counted for one warning. A page limit stops one data stream's read and the read goes on with the next; the
/// observation limit stops the reads of Observations. When the settings request `HistoricalLocations`,
/// those of the window of each Thing a listed data stream belongs to are read last, by ascending
/// Thing id, oldest first, each with its Locations; a page limit stops one Thing's read, and the
/// observation limit, which counts Observations only, stops none.
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
    let mut things = ListedThings::default();
    let discoveries = discover_streams(reader, service, metadata_discovery_request, |kind, served| {
        datastreams.add(kind, &served, window);
        if settings.historical_locations() == HistoricalLocations::Requested {
            things.add(&served);
        }
        snapshot.datastream(&DatastreamLine::new(kind, served)).map_err(unwritable(path))
    })?;
    for (_, end) in discoveries {
        if let TraversalEnd::LimitReached(limit) = end {
            warnings.limit_reached(limit);
        }
    }
    let mut sink = ObservationSink::new(snapshot.into_observations(), service, path);
    // A range read keeps nothing between cycles, so no Observation is seen before it is read.
    let unseen = SeenEntities::default();
    for datastream in datastreams.into_ids() {
        let request = observations_request(service.root(), &datastream, window, settings.page_size(), settings.features_of_interest());
        let end = read_observations(reader, request, settings.limits().pages(), &datastream, &unseen, &mut sink, |_, _| {})?;
        if next_stream(end, &mut warnings).is_break() {
            break;
        }
    }
    let mut sink = sink.into_historical_locations(&mut warnings);
    for thing in things.into_ids() {
        read_historical_locations(reader, service, &thing, window, &unseen, &mut sink, &mut warnings)?;
    }
    sink.finish()?;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use crate::{
        collection_warnings::CollectionWarnings,
        sensorthings::{
            features_of_interest_unserved::{FeaturesOfInterestUnserved, UnservedObservation},
            limit_reached::{CollectLimit, LimitReached},
            range_empty::RangeEmpty,
            test_collection::{RUNNING, collect, collector, root, snapshot_lines},
        },
    };
    use cassiopeia_common::request_header::RequestHeaders;
    use cassiopeia_sensorthings::{
        entity_id::EntityId,
        observation_name::ObservationName,
        read_kind::ReadKind,
        read_window::ReadWindow,
        settings::collect_limits::{ObservationLimit, PageLimit},
        snapshot::header::SnapshotHeader,
        stream_id::StreamId,
    };
    use chrono::{DateTime, Utc};
    use mockito::{Matcher, Mock, ServerGuard};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{num::NonZeroU64, slice};
    use url::Url;

    /// The path and query of the metadata discovery request with the default settings.
    const METADATA: &str = "/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc";

    /// The bounds of a `last: "24h"` range in the cycle the tests collect in.
    const LAST_DAY: (&str, &str) = ("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z");

    /// The settings of a range read of `range`, a JSON object, with the members `extra` beside it.
    fn range_read(range: &str, extra: &str) -> String {
        format!(r#"{{"read": {{"range": {range}}}{extra}}}"#)
    }

    /// The path and query of the observations request of the data stream `id` between `bounds`.
    fn observations_path(id: u64, bounds: (&str, &str)) -> String {
        format!(
            "/v1.1/Datastreams({id})/Observations?$filter=phenomenonTime%20ge%20{}%20and%20phenomenonTime%20lt%20{}&$orderby=phenomenonTime%20asc,id%20asc&$select=id,phenomenonTime,result,resultTime,validTime,resultQuality,parameters&$top=1000",
            bounds.0, bounds.1
        )
    }

    /// The path and query of the observations request of the data stream `id` between `bounds` that
    /// requests every Observation with its `FeatureOfInterest`.
    fn observations_with_features_path(id: u64, bounds: (&str, &str)) -> String {
        format!(
            "/v1.1/Datastreams({id})/Observations?$filter=phenomenonTime%20ge%20{}%20and%20phenomenonTime%20lt%20{}&$orderby=phenomenonTime%20asc,id%20asc&$expand=FeatureOfInterest($select=id,name,description,properties,encodingType,feature)&$select=id,phenomenonTime,result,resultTime,validTime,resultQuality,parameters&$top=1000",
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
    fn a_range_read_keeps_each_sensor_inside_its_datastream_line() {
        let mut server = mockito::Server::new();
        let datastream = json!({
            "@iot.id": 1,
            "name": "stream 1",
            "Sensor": {"@iot.id": 7, "name": "PM-200", "metadata": "https://sensors.example.org/pm-200.pdf"},
        });
        let mocks = [
            serve(&mut server, METADATA, page(slice::from_ref(&datastream), None)),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11]), None)),
        ];

        let (result, payloads) = collect(collector(&server, &range_read(r#"{"last": "24h"}"#, ""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(lines[1], json!({ "datastream": datastream }));
        assert_eq!(observation_lines(&lines), [(json!(11), json!(1))]);
    }

    #[test]
    fn a_range_read_of_features_of_interest_keeps_each_inside_its_observation() {
        let mut server = mockito::Server::new();
        let observation = json!({
            "@iot.id": 11,
            "phenomenonTime": "2026-11-12T01:00:00Z",
            "result": 11,
            "FeatureOfInterest": {
                "@iot.id": 4711,
                "name": "Line 12, Stationsplein",
                "encodingType": "application/geo+json",
                "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]},
            },
        });
        let mocks = [
            discovery(&mut server, &[1]),
            serve(
                &mut server,
                &observations_with_features_path(1, LAST_DAY),
                page(slice::from_ref(&observation), None),
            ),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "featuresOfInterest": true"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        assert_eq!(result.unwrap(), CollectionWarnings::default());
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(lines[2], json!({"observation": observation, "datastream": 1}));
    }

    /// Served Observations `ids`, as [`observations`] serves them, each with the `FeatureOfInterest`
    /// member `feature`.
    fn observations_with(ids: &[u64], feature: &Value) -> Vec<Value> {
        observations(ids)
            .into_iter()
            .map(|mut observation| {
                observation["FeatureOfInterest"] = feature.clone();
                observation
            })
            .collect()
    }

    #[test]
    fn a_range_read_of_a_server_that_serves_no_feature_of_interest_warns_once_naming_the_first_observation() {
        let mut server = mockito::Server::new();
        let mut first = observations(&[11]);
        first.extend(observations_with(&[12], &Value::Null));
        let mocks = [
            discovery(&mut server, &[1, 2]),
            serve(&mut server, &observations_with_features_path(1, LAST_DAY), page(&first, None)),
            serve(&mut server, &observations_with_features_path(2, LAST_DAY), page(&observations(&[21]), None)),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "featuresOfInterest": true"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        let warnings = result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            warnings.unserved_features_of_interest(),
            [FeaturesOfInterestUnserved::new(
                root(&server),
                UnservedObservation {
                    observation: ObservationName::Id(EntityId::Integer(11)),
                    datastream: StreamId::Datastream(EntityId::Integer(1)),
                },
                NonZeroU64::new(3).unwrap(),
            )]
        );
        let lines = snapshot_lines(payloads);
        assert_eq!(observation_lines(&lines), [(json!(11), json!(1)), (json!(12), json!(1)), (json!(21), json!(2))]);
        assert_eq!(lines[4], json!({"observation": first[1], "datastream": 1}));
    }

    #[test]
    fn a_range_read_counts_only_the_observations_it_writes_without_their_feature_of_interest() {
        let mut server = mockito::Server::new();
        let feature = json!({"@iot.id": 4711, "encodingType": "application/geo+json", "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]}});
        let mut items = observations_with(&[11], &feature);
        items.extend(observations(&[12, 13]));
        let mocks = [
            discovery(&mut server, &[1]),
            serve(&mut server, &observations_with_features_path(1, LAST_DAY), page(&items, None)),
        ];

        let (result, _) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "featuresOfInterest": true, "limits": {"observations": 2}"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        let warnings = result.unwrap();
        mocks.iter().for_each(Mock::assert);
        let [unserved] = warnings.unserved_features_of_interest() else {
            panic!("expected one warning: {warnings:?}");
        };
        assert_eq!(unserved.first().to_string(), "Observation 12 of Datastream 1");
        assert_eq!(unserved.count().get(), 1);
    }

    #[test]
    fn a_range_read_that_omits_features_of_interest_sends_no_expansion() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[1]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11]), None)),
        ];

        let (result, _) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "featuresOfInterest": false"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        assert_eq!(result.unwrap(), CollectionWarnings::default());
        mocks.iter().for_each(Mock::assert);
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

    /// The path and query of the metadata discovery of every `MultiDatastream`.
    const MULTI_METADATA: &str = "/v1.1/MultiDatastreams?$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc";

    /// The path and query of the observations request of the `MultiDatastream` `id` between
    /// `bounds`.
    fn multi_observations_path(id: u64, bounds: (&str, &str)) -> String {
        observations_path(id, bounds).replacen("/v1.1/Datastreams(", "/v1.1/MultiDatastreams(", 1)
    }

    /// Served `MultiDatastream` Observations `ids`, each with a two-value result.
    fn multi_observations(ids: &[u64]) -> Vec<Value> {
        ids.iter()
            .map(|id| json!({"@iot.id": id, "phenomenonTime": format!("2026-11-12T{:02}:00:00Z", id % 10), "result": [id, 270]}))
            .collect()
    }

    /// Each observation line of a snapshot as the id of its Observation and the member that names
    /// its data stream, in file order.
    fn named_streams(lines: &[Value]) -> Vec<(Value, Value)> {
        lines
            .iter()
            .filter_map(|line| line.as_object())
            .filter(|line| line.contains_key("observation"))
            .map(|line| {
                let stream: serde_json::Map<String, Value> = line
                    .iter()
                    .filter(|(key, _)| *key != "observation")
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                (line["observation"]["@iot.id"].clone(), Value::Object(stream))
            })
            .collect()
    }

    #[test]
    fn multi_datastreams_are_read_after_datastreams_each_kind_by_ascending_id() {
        let mut server = mockito::Server::new();
        let multi: Vec<Value> = [4, 2].iter().map(|id| json!({"@iot.id": id, "name": format!("ws-{id}")})).collect();
        let mocks = [
            discovery(&mut server, &[5]),
            serve(&mut server, MULTI_METADATA, page(&multi, None)),
            serve(&mut server, &observations_path(5, LAST_DAY), page(&observations(&[51]), None)),
            serve(&mut server, &multi_observations_path(2, LAST_DAY), page(&multi_observations(&[21]), None)),
            serve(
                &mut server,
                &multi_observations_path(4, LAST_DAY),
                page(&multi_observations(&[41, 42, 43]), None),
            ),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "multiDatastreams": true"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(lines[2], json!({"multiDatastream": {"@iot.id": 4, "name": "ws-4"}}));
        assert_eq!(
            named_streams(&lines),
            [
                (json!(51), json!({"datastream": 5})),
                (json!(21), json!({"multiDatastream": 2})),
                (json!(41), json!({"multiDatastream": 4})),
                (json!(42), json!({"multiDatastream": 4})),
                (json!(43), json!({"multiDatastream": 4})),
            ]
        );
    }

    #[test]
    fn one_observation_limit_counts_the_observations_of_both_kinds() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery(&mut server, &[1]),
            serve(&mut server, MULTI_METADATA, page(&[json!({"@iot.id": 4})], None)),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[1, 2]), None)),
            serve(&mut server, &multi_observations_path(4, LAST_DAY), page(&multi_observations(&[3, 4]), None)),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "multiDatastreams": true, "limits": {"observations": 3}"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            result.unwrap().limits_reached(),
            [LimitReached::new(
                root(&server),
                CollectLimit::Observations(ObservationLimit::try_from(3).unwrap()),
                Url::parse(&format!("{}{}", server.url(), multi_observations_path(4, LAST_DAY))).unwrap(),
            )]
        );
        let written: Vec<Value> = named_streams(&snapshot_lines(payloads)).into_iter().map(|(id, _)| id).collect();
        assert_eq!(written, [json!(1), json!(2), json!(3)]);
    }

    /// The path and query of the `HistoricalLocations` request of the Thing addressed as `key`, such as
    /// `(12512)` or `('bus-12')`, between `bounds`, `top` a page.
    fn historical_locations_path(key: &str, bounds: (&str, &str), top: u16) -> String {
        format!(
            "/v1.1/Things{key}/HistoricalLocations?$filter=time%20ge%20{}%20and%20time%20lt%20{}&$expand=Locations&$orderby=time%20asc,id%20asc&$top={top}",
            bounds.0, bounds.1
        )
    }

    /// Served `HistoricalLocations` `ids`, each known at a minute past 09:00 on 2026-11-12 as late as
    /// its id, at one `GeoJSON` Location.
    fn historical_locations(ids: &[u64]) -> Vec<Value> {
        ids.iter()
            .map(|id| {
                json!({
                    "@iot.id": id,
                    "time": format!("2026-11-12T09:{:02}:00Z", id % 60),
                    "Locations": [{
                        "@iot.id": 36190,
                        "encodingType": "application/geo+json",
                        "location": {"type": "Point", "coordinates": [5.4797, 51.4433]},
                    }],
                })
            })
            .collect()
    }

    /// Serves the metadata discovery at `path_and_query` with the Datastreams `streams`, each an id
    /// and the id of its Thing.
    fn discovery_of_things(server: &mut ServerGuard, path_and_query: &str, streams: &[(u64, Value)]) -> Mock {
        let datastreams: Vec<Value> = streams
            .iter()
            .map(|(id, thing)| json!({"@iot.id": id, "name": format!("stream {id}"), "Thing": {"@iot.id": thing}}))
            .collect();
        serve(server, path_and_query, page(&datastreams, None))
    }

    /// Each `historicalLocation` line of a snapshot as the id of its `HistoricalLocation` and of its
    /// Thing, in file order.
    fn historical_location_lines(lines: &[Value]) -> Vec<(Value, Value)> {
        lines
            .iter()
            .filter(|line| line.get("historicalLocation").is_some())
            .map(|line| (line["historicalLocation"]["@iot.id"].clone(), line["thing"].clone()))
            .collect()
    }

    /// The settings of a `last: "24h"` range read of `HistoricalLocations`, with the members `extra`.
    fn history_read(extra: &str) -> String {
        range_read(r#"{"last": "24h"}"#, &format!(r#", "historicalLocations": true{extra}"#))
    }

    #[test]
    fn the_historical_locations_of_each_listed_thing_follow_the_observations_by_ascending_thing_id() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery_of_things(&mut server, METADATA, &[(1, json!("bus-12")), (2, json!(12512)), (3, json!(12512))]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11]), None)),
            serve(&mut server, &observations_path(2, LAST_DAY), page(&observations(&[21]), None)),
            serve(&mut server, &observations_path(3, LAST_DAY), page(&observations(&[31]), None)),
            serve(
                &mut server,
                &historical_locations_path("(12512)", LAST_DAY, 1000),
                page(&historical_locations(&[4, 34, 52]), None),
            ),
            serve(
                &mut server,
                &historical_locations_path("('bus-12')", LAST_DAY, 1000),
                page(&historical_locations(&[7]), None),
            ),
        ];

        let (result, payloads) = collect(collector(&server, &history_read(""), RequestHeaders::default(), &RUNNING));

        assert_eq!(result.unwrap(), CollectionWarnings::default());
        mocks.iter().for_each(Mock::assert);
        let lines = snapshot_lines(payloads);
        assert_eq!(observation_lines(&lines), [(json!(11), json!(1)), (json!(21), json!(2)), (json!(31), json!(3))]);
        assert_eq!(
            historical_location_lines(&lines),
            [
                (json!(4), json!(12512)),
                (json!(34), json!(12512)),
                (json!(52), json!(12512)),
                (json!(7), json!("bus-12")),
            ]
        );
        assert_eq!(lines[7], json!({"historicalLocation": historical_locations(&[4])[0], "thing": 12512}));
        assert!(lines[7..].iter().all(|line| line.get("historicalLocation").is_some()), "{lines:?}");
    }

    #[test]
    fn no_historical_location_is_requested_unless_the_settings_request_them() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery_of_things(&mut server, METADATA, &[(1, json!(12512))]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11]), None)),
        ];
        let untouched = server.mock("GET", Matcher::Regex(r"^/v1\.1/Things".to_owned())).expect(0).create();

        let (result, payloads) = collect(collector(
            &server,
            &range_read(r#"{"last": "24h"}"#, r#", "historicalLocations": false"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        untouched.assert();
        assert_eq!(historical_location_lines(&snapshot_lines(payloads)), []);
    }

    #[test]
    fn the_page_limit_ends_a_thing_s_read_and_names_the_next_link() {
        let mut server = mockito::Server::new();
        let next = format!("{}/v1.1/Things(12512)/HistoricalLocations?$skip=2", server.url());
        let mocks = [
            discovery_of_things(&mut server, &METADATA.replace("$top=1000", "$top=2"), &[(1, json!(12512))]),
            serve(&mut server, &observations_path(1, LAST_DAY).replace("$top=1000", "$top=2"), page(&[], None)),
            serve(
                &mut server,
                &historical_locations_path("(12512)", LAST_DAY, 2),
                page(&historical_locations(&[4, 34]), Some(&next)),
            ),
        ];
        let unfollowed = server.mock("GET", Matcher::Regex(r"\$skip=2$".to_owned())).expect(0).create();

        let (result, payloads) = collect(collector(
            &server,
            &history_read(r#", "pageSize": 2, "limits": {"pages": 1}"#),
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
                Url::parse(&next).unwrap(),
            )]
        );
        assert_eq!(
            historical_location_lines(&snapshot_lines(payloads)),
            [(json!(4), json!(12512)), (json!(34), json!(12512))]
        );
    }

    #[test]
    fn the_observation_limit_stops_no_historical_location() {
        let mut server = mockito::Server::new();
        let mocks = [
            discovery_of_things(&mut server, METADATA, &[(1, json!(12512))]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&observations(&[11, 12]), None)),
            serve(
                &mut server,
                &historical_locations_path("(12512)", LAST_DAY, 1000),
                page(&historical_locations(&[4, 34]), None),
            ),
        ];

        let (result, payloads) = collect(collector(
            &server,
            &history_read(r#", "limits": {"observations": 1}"#),
            RequestHeaders::default(),
            &RUNNING,
        ));

        mocks.iter().for_each(Mock::assert);
        let limits: Vec<CollectLimit> = result.unwrap().limits_reached().iter().map(LimitReached::limit).collect();
        assert_eq!(limits, [CollectLimit::Observations(ObservationLimit::try_from(1).unwrap())]);
        let lines = snapshot_lines(payloads);
        assert_eq!(observation_lines(&lines), [(json!(11), json!(1))]);
        assert_eq!(historical_location_lines(&lines), [(json!(4), json!(12512)), (json!(34), json!(12512))]);
    }

    #[test]
    fn a_historical_location_repeated_across_pages_is_written_once() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Things(12512)/HistoricalLocations?$skip=2", server.url());
        let mocks = [
            discovery_of_things(&mut server, METADATA, &[(1, json!(12512))]),
            serve(&mut server, &observations_path(1, LAST_DAY), page(&[], None)),
            serve(
                &mut server,
                &historical_locations_path("(12512)", LAST_DAY, 1000),
                page(&historical_locations(&[4, 34]), Some(&second)),
            ),
            serve(
                &mut server,
                "/v1.1/Things(12512)/HistoricalLocations?$skip=2",
                page(&historical_locations(&[34, 52]), None),
            ),
        ];

        let (result, payloads) = collect(collector(&server, &history_read(""), RequestHeaders::default(), &RUNNING));

        result.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(
            historical_location_lines(&snapshot_lines(payloads)),
            [(json!(4), json!(12512)), (json!(34), json!(12512)), (json!(52), json!(12512))]
        );
    }
}

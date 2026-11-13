use crate::{
    collection_warnings::CollectionWarnings,
    error::CollectorError,
    sensorthings::{
        cursor_cycle::CursorCycle,
        datastreams_missing::DatastreamsMissing,
        observation_read::{ObservationSink, next_stream, read_observations},
        page_reader::PageReader,
        snapshot_failure::unwritable,
        traversal::{TraversalEnd, traverse},
    },
};
use cassiopeia_common::cycle_start::CycleStart;
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    incremental_datastreams::IncrementalDatastreams,
    phenomenon_time::PhenomenonTime,
    request::{discovery_request::metadata_discovery_request, observations_request::observations_request},
    service::SensorThingsService,
    settings::incremental_read::IncrementalRead,
    snapshot::{line::DatastreamLine, writer::SnapshotWriter},
};
use cassiopeia_sensorthings_cursor::{cursor_update::CursorUpdate, position::Position, seen_observations::SeenObservations};
use std::{io::Write, ops::ControlFlow, path::Path};

/// Reads every reading the data streams `service` selects gained since `cursor`'s committed
/// positions into `snapshot`, after its header, in the cycle that started at `cycle_start`, leaves
/// the cursor's update in its slot, and returns what the read met that the cycle carries on past.
///
/// The metadata discovery comes first, and every Datastream it serves is written as it was served.
/// Each data stream is then read on its own, oldest reading first, from a lookback behind its
/// high-water mark, or from the initial point when the cursor holds no position for it, to the cycle
/// start, so a reading stamped at or after the cycle start waits for a later cycle. The data streams
/// furthest behind are read first. An Observation the position has seen, or this read already wrote,
/// is dropped. A page limit stops one data stream's read and the read goes on with the next; the
/// observation limit stops the whole read, and what each data stream wrote up to there moves its
/// position, so the next cycle goes on from there.
///
/// The update records every data stream discovery listed with the Observations written for it, each
/// by its id and the instant its `phenomenonTime` stands for. An Observation without a readable id
/// or `phenomenonTime` is written but not recorded. The update is left only once the whole read has
/// succeeded; the read never commits it.
///
/// # Errors
///
/// Returns every [`CollectorError`] a traversal fails with, and [`CollectorError::Io`] when the
/// snapshot at `path` cannot be written.
pub(crate) fn read_incremental<W: Write>(
    reader: &mut PageReader,
    service: &SensorThingsService,
    incremental: &IncrementalRead,
    cursor: CursorCycle,
    cycle_start: CycleStart,
    mut snapshot: SnapshotWriter<W>,
    path: &Path,
) -> Result<CollectionWarnings, CollectorError> {
    let settings = service.settings();
    let start = cycle_start.instant();
    let (positions, slot) = cursor.into_parts();
    let mut update = CursorUpdate::new(start, incremental.lookback().get());
    let mut datastreams = IncrementalDatastreams::new(start);
    let discovery = traverse(
        reader,
        metadata_discovery_request(service.root(), settings),
        settings.limits().pages(),
        |datastream| {
            if let Some(id) = EntityId::of(&datastream) {
                let from = incremental.start(positions.get(&id).map(Position::high_water), start);
                // The update and the reads each keep the id.
                datastreams.add(id.clone(), &datastream, from);
                update.record(id, SeenObservations::default());
            }
            snapshot.datastream(&DatastreamLine::new(datastream)).map_err(unwritable(path))?;
            Ok::<_, CollectorError>(ControlFlow::Continue(()))
        },
    )?;
    let mut warnings = CollectionWarnings::default();
    match discovery {
        TraversalEnd::LastPage => {
            if let Some(missing) = DatastreamsMissing::of(service.root(), incremental.cursor(), &positions, &update, start) {
                warnings.datastreams_missing(missing);
            }
        }
        TraversalEnd::LimitReached(limit) => {
            warnings.limit_reached(limit);
            update.listing_cut_short();
        }
    }
    let mut sink = ObservationSink::new(snapshot.into_observations(), settings.limits().observations(), path);
    let unseen = SeenObservations::default();
    for (datastream, window) in datastreams.into_reads() {
        let request = observations_request(service.root(), &datastream, window, settings.page_size());
        let seen = positions.get(&datastream).map_or(&unseen, Position::seen);
        let mut written = SeenObservations::default();
        let end = read_observations(reader, request, settings.limits().pages(), &datastream, seen, &mut sink, |id, observation| {
            if let Some(time) = PhenomenonTime::of(observation) {
                // The record of what was written keeps the id, while the read keeps it to drop repeats.
                written.insert(id.clone(), time.instant());
            }
        })?;
        update.record(datastream, written);
        if next_stream(end, &mut warnings).is_break() {
            break;
        }
    }
    sink.finish()?;
    slot.fill(update);
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::CollectorError,
        sensorthings::{
            cursor_cycle::CursorCycle,
            cursor_update_slot::CursorUpdateSlot,
            limit_reached::{CollectLimit, LimitReached},
            test_collection::{RUNNING, collect, collector, cycle_start, resuming_collector, root, snapshot_lines},
        },
    };
    use cassiopeia_common::request_header::RequestHeaders;
    use cassiopeia_sensorthings::{entity_id::EntityId, read_kind::ReadKind, settings::collect_limits::ObservationLimit, snapshot::header::SnapshotHeader};
    use cassiopeia_sensorthings_cursor::{cursor_update::CursorUpdate, positions::Positions, seen_observations::SeenObservations};
    use chrono::{DateTime, Utc};
    use mockito::{Matcher, Mock, Request, ServerGuard};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use url::Url;

    /// The path and query of the metadata discovery request with the default settings.
    const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=1000";

    /// The settings of an incremental read through the cursor `rivm-air`, with the members `extra`
    /// beside `read`.
    fn incremental(extra: &str) -> String {
        format!(r#"{{"read": {{"incremental": {{"cursor": "rivm-air"}}}}{extra}}}"#)
    }

    /// The instant `time` on 2026-11-12, in UTC, such as `09:30` or `10:00:05`.
    fn at(time: &str) -> DateTime<Utc> {
        let time = if time.len() == 5 { format!("{time}:00") } else { time.to_owned() };
        DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}Z")).unwrap().to_utc()
    }

    fn id(id: i64) -> EntityId {
        EntityId::Integer(id)
    }

    /// The Observations `observations`, each an id with the time it was written for.
    fn seen(observations: &[(i64, &str)]) -> SeenObservations {
        SeenObservations::from(
            observations
                .iter()
                .map(|(observation, time)| (id(*observation), at(time)))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    /// The positions a cycle that started at 09:00 leaves for the data streams `held`, each listed
    /// with the Observations written for it, its latest the high-water mark.
    fn positions(held: &[(i64, &[(i64, &str)])]) -> Positions {
        let mut update = CursorUpdate::new(at("09:00"), Duration::from_hours(1));
        for (datastream, written) in held {
            update.record(id(*datastream), seen(written));
        }
        update.apply(Positions::default())
    }

    /// `held` after a cycle that started at `cycle_start` listed none of its data streams.
    fn missed(held: Positions, cycle_start: &str) -> Positions {
        CursorUpdate::new(at(cycle_start), Duration::from_hours(1)).apply(held)
    }

    /// The paths and queries a mock server answered, in the order it answered them.
    #[derive(Clone, Default)]
    struct Answered(Arc<Mutex<Vec<String>>>);

    impl Answered {
        fn requests(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    /// The `phenomenonTime` bounds the observations request `path_and_query` asks for.
    fn bounds(path_and_query: &str) -> (DateTime<Utc>, DateTime<Utc>) {
        let instant = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().to_utc();
        let (_, filter) = path_and_query.split_once("phenomenonTime%20ge%20").unwrap();
        let (from, rest) = filter.split_once("%20and%20phenomenonTime%20lt%20").unwrap();
        let (to, _) = rest.split_once('&').unwrap();
        (instant(from), instant(to))
    }

    /// The `phenomenonTime` bounds every observations request answered asks for, in the order answered.
    fn requested_bounds(answered: &Answered) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        answered.requests().iter().map(|request| bounds(request)).collect()
    }

    /// Serves the metadata discovery with Datastreams `ids`, each without a `phenomenonTime`, once.
    fn discovery(server: &mut ServerGuard, ids: &[i64]) -> Mock {
        let datastreams: Vec<Value> = ids.iter().map(|id| json!({"@iot.id": id, "name": format!("stream {id}")})).collect();
        server
            .mock("GET", METADATA)
            .with_body(json!({ "value": datastreams }).to_string())
            .expect(1)
            .create()
    }

    /// Serves the Observations `observations` of the data stream `datastream`, each an id and a time,
    /// as a server does, answering each request with those in its `phenomenonTime` bounds, oldest
    /// first, and recording it in `answered`.
    fn serve_observations(server: &mut ServerGuard, datastream: i64, observations: &[(i64, &str)], answered: &Answered) -> Mock {
        let held: Vec<(DateTime<Utc>, Value)> = observations
            .iter()
            .map(|(observation, time)| {
                (
                    at(time),
                    json!({"@iot.id": observation, "phenomenonTime": at(time).to_rfc3339(), "result": observation}),
                )
            })
            .collect();
        let answered = Arc::clone(&answered.0);
        server
            .mock("GET", Matcher::Regex(format!(r"^/v1\.1/Datastreams\({datastream}\)/Observations\?")))
            .with_body_from_request(move |request: &Request| {
                let path = request.path_and_query().to_owned();
                let (from, to) = bounds(&path);
                answered.lock().unwrap().push(path);
                let mut answer: Vec<&(DateTime<Utc>, Value)> = held.iter().filter(|(time, _)| from <= *time && *time < to).collect();
                answer.sort_by_key(|(time, _)| *time);
                json!({ "value": answer.iter().map(|(_, observation)| observation).collect::<Vec<_>>() })
                    .to_string()
                    .into_bytes()
            })
            .create()
    }

    /// Reads the mock server incrementally with `settings` from `committed`, returning what the read
    /// returned, the ids of the Observations it wrote in file order, the header's read, and the
    /// positions its update moves `committed` to.
    fn read(server: &ServerGuard, settings: &str, committed: Positions) -> (Result<Vec<Value>, CollectorError>, Positions, ReadKind) {
        let slot = CursorUpdateSlot::default();
        let (result, payloads) = collect(resuming_collector(server, settings, CursorCycle::new(committed.clone(), slot.clone())));
        match result {
            Ok(_) => {
                let lines = snapshot_lines(payloads);
                let read = SnapshotHeader::from_line(&lines[0].to_string()).unwrap().read();
                let written = lines
                    .iter()
                    .filter_map(|line| line.get("observation"))
                    .map(|observation| observation["@iot.id"].clone())
                    .collect();
                (Ok(written), slot.take().unwrap().apply(committed), read)
            }
            Err(error) => {
                assert_eq!(slot.take(), None);
                (Err(error), committed, ReadKind::Incremental)
            }
        }
    }

    #[test]
    fn a_new_stream_starts_at_the_initial_point() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        let mocks = [
            discovery(&mut server, &[5]),
            serve_observations(&mut server, 5, &[(1, "09:00"), (2, "09:30")], &answered),
        ];

        let (written, positions, read) = read(&server, &incremental(""), Positions::default());

        mocks.iter().for_each(Mock::assert);
        assert_eq!(read, ReadKind::Incremental);
        assert_eq!(written.unwrap(), [json!(1), json!(2)]);
        assert_eq!(
            requested_bounds(&answered),
            [(DateTime::parse_from_rfc3339("2026-11-11T10:00:00Z").unwrap().to_utc(), at("10:00"))]
        );
        let position = positions.get(&id(5)).unwrap();
        assert_eq!(position.high_water(), at("09:30"));
        assert_eq!(position.seen(), &seen(&[(1, "09:00"), (2, "09:30")]));
    }

    #[test]
    fn a_known_stream_resumes_a_lookback_behind_its_high_water() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        let mocks = [discovery(&mut server, &[5]), serve_observations(&mut server, 5, &[], &answered)];

        let (written, _, _) = read(&server, &incremental(""), positions(&[(5, &[(8, "09:00")])]));

        written.unwrap();
        mocks.iter().for_each(Mock::assert);
        assert_eq!(requested_bounds(&answered), [(at("08:00"), at("10:00"))]);
    }

    #[test]
    fn seen_observations_are_dropped_and_late_ones_kept() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[5]);
        serve_observations(&mut server, 5, &[(7, "08:30"), (8, "09:00"), (9, "08:45"), (10, "09:30")], &answered);

        let (written, positions, _) = read(&server, &incremental(""), positions(&[(5, &[(7, "08:30"), (8, "09:00")])]));

        assert_eq!(written.unwrap(), [json!(9), json!(10)]);
        let position = positions.get(&id(5)).unwrap();
        assert_eq!(position.high_water(), at("09:30"));
        assert_eq!(position.seen(), &seen(&[(7, "08:30"), (8, "09:00"), (9, "08:45"), (10, "09:30")]));
    }

    #[test]
    fn a_stream_without_new_readings_keeps_its_position() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[5]);
        serve_observations(&mut server, 5, &[], &answered);

        let (written, positions, _) = read(&server, &incremental(""), positions(&[(5, &[(7, "08:30"), (8, "09:00")])]));

        assert_eq!(written.unwrap(), Vec::<Value>::new());
        let position = positions.get(&id(5)).unwrap();
        assert_eq!(position.high_water(), at("09:00"));
        assert_eq!(position.seen(), &seen(&[(7, "08:30"), (8, "09:00")]));
        assert_eq!(position.last_listed(), cycle_start().instant());
    }

    #[test]
    fn the_stalest_stream_is_read_first() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[1, 2, 3]);
        for datastream in [1, 2, 3] {
            serve_observations(&mut server, datastream, &[], &answered);
        }
        let held = positions(&[(1, &[(11, "09:30")]), (2, &[(21, "07:00")])]);

        let (written, _, _) = read(&server, &incremental(""), held);

        written.unwrap();
        let order: Vec<&str> = answered
            .requests()
            .iter()
            .map(|request| ["(1)", "(2)", "(3)"].into_iter().find(|key| request.contains(key)).unwrap())
            .collect();
        assert_eq!(order, ["(3)", "(2)", "(1)"]);
    }

    #[test]
    fn a_missing_stream_keeps_its_position_and_is_reported() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[1]);
        serve_observations(&mut server, 1, &[], &answered);
        let untouched = server.mock("GET", Matcher::Regex(r"^/v1\.1/Datastreams\(2\)/".to_owned())).expect(0).create();
        let held = positions(&[(1, &[(11, "09:00")]), (2, &[(4, "07:30"), (5, "08:00")])]);
        let slot = CursorUpdateSlot::default();

        let (result, payloads) = collect(resuming_collector(&server, &incremental(""), CursorCycle::new(held.clone(), slot.clone())));

        untouched.assert();
        snapshot_lines(payloads);
        let warnings = result.unwrap();
        let [missing] = warnings.missing_datastreams() else {
            panic!("expected one warning, got {:?}", warnings.missing_datastreams());
        };
        assert_eq!((missing.root(), missing.cursor().as_str()), (&root(&server), "rivm-air"));
        assert_eq!(
            (missing.named().as_slice(), missing.count().get(), missing.since()),
            ([id(2)].as_slice(), 1, cycle_start().instant())
        );
        let positions = slot.take().unwrap().apply(held);
        let kept = positions.get(&id(2)).unwrap();
        assert_eq!(kept.missing_since(), Some(cycle_start().instant()));
        assert_eq!((kept.high_water(), kept.seen()), (at("08:00"), &seen(&[(4, "07:30"), (5, "08:00")])));
    }

    #[test]
    fn a_stream_listed_again_clears_its_missing_mark() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[2]);
        serve_observations(&mut server, 2, &[], &answered);

        let held = missed(positions(&[(2, &[(5, "08:00")])]), "09:30");
        assert_eq!(held.get(&id(2)).unwrap().missing_since(), Some(at("09:30")));

        let (written, positions, _) = read(&server, &incremental(""), held);

        written.unwrap();
        assert_eq!(positions.get(&id(2)).unwrap().missing_since(), None);
    }

    #[test]
    fn a_limit_mid_stream_defers_the_rest_to_the_next_cycle() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        let held: Vec<(i64, String)> = (0..10).map(|minute| (100 + minute, format!("09:{minute:02}"))).collect();
        let held: Vec<(i64, &str)> = held.iter().map(|(observation, time)| (*observation, time.as_str())).collect();
        server
            .mock("GET", METADATA)
            .with_body(json!({"value": [{"@iot.id": 5}]}).to_string())
            .expect(2)
            .create();
        serve_observations(&mut server, 5, &held, &answered);
        let limited = incremental(r#", "limits": {"observations": 6}"#);

        let (first, after_first, _) = read(&server, &limited, Positions::default());
        let (second, after_second, _) = read(&server, &limited, after_first.clone());

        assert_eq!(first.unwrap(), (100..106).map(|observation| json!(observation)).collect::<Vec<_>>());
        assert_eq!(after_first.get(&id(5)).unwrap().high_water(), at("09:05"));
        assert_eq!(second.unwrap(), (106..110).map(|observation| json!(observation)).collect::<Vec<_>>());
        assert_eq!(after_second.get(&id(5)).unwrap().high_water(), at("09:09"));
    }

    #[test]
    fn the_observation_limit_is_reported_and_keeps_what_was_written() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[5]);
        serve_observations(&mut server, 5, &[(1, "09:00"), (2, "09:10"), (3, "09:20")], &answered);
        let slot = CursorUpdateSlot::default();

        let (result, payloads) = collect(resuming_collector(
            &server,
            &incremental(r#", "limits": {"observations": 2}"#),
            CursorCycle::new(Positions::default(), slot.clone()),
        ));

        snapshot_lines(payloads);
        let reached = result.unwrap();
        assert_eq!(reached.limits_reached().len(), 1);
        assert_eq!(
            reached.limits_reached()[0].limit(),
            CollectLimit::Observations(ObservationLimit::try_from(2).unwrap())
        );
        let positions = slot.take().unwrap().apply(Positions::default());
        assert_eq!(positions.get(&id(5)).unwrap().seen(), &seen(&[(1, "09:00"), (2, "09:10")]));
    }

    #[test]
    fn readings_at_or_after_the_cycle_start_wait_for_the_next_cycle() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        discovery(&mut server, &[5]);
        serve_observations(&mut server, 5, &[(1, "09:59:59"), (2, "10:00:05")], &answered);

        let (written, positions, _) = read(&server, &incremental(""), Positions::default());

        assert_eq!(written.unwrap(), [json!(1)]);
        assert!(
            answered
                .requests()
                .iter()
                .all(|request| request.contains("phenomenonTime%20lt%202026-11-12T10:00:00Z&")),
            "{:?}",
            answered.requests()
        );
        assert_eq!(positions.get(&id(5)).unwrap().high_water(), at("09:59:59"));
    }

    #[test]
    fn a_cut_short_discovery_marks_no_stream_missing() {
        let mut server = mockito::Server::new();
        let answered = Answered::default();
        let next = format!("{}/v1.1/Datastreams?$skip=1", server.url());
        server
            .mock("GET", METADATA)
            .with_body(json!({"value": [{"@iot.id": 1}], "@iot.nextLink": next}).to_string())
            .expect(1)
            .create();
        serve_observations(&mut server, 1, &[], &answered);
        let held = positions(&[(1, &[(11, "09:00")]), (2, &[(21, "08:00")])]);
        let slot = CursorUpdateSlot::default();

        let (result, payloads) = collect(resuming_collector(
            &server,
            &incremental(r#", "limits": {"pages": 1}"#),
            CursorCycle::new(held.clone(), slot.clone()),
        ));

        snapshot_lines(payloads);
        let warnings = result.unwrap();
        assert_eq!(warnings.missing_datastreams(), []);
        assert_eq!(
            warnings.limits_reached().iter().map(LimitReached::next).collect::<Vec<_>>(),
            [&Url::parse(&next).unwrap()]
        );
        assert_eq!(slot.take().unwrap().apply(held).get(&id(2)).unwrap().missing_since(), None);
    }

    #[test]
    fn an_observation_without_a_readable_time_is_written_but_not_recorded() {
        let mut server = mockito::Server::new();
        discovery(&mut server, &[5]);
        server
            .mock("GET", Matcher::Regex(r"^/v1\.1/Datastreams\(5\)/Observations\?".to_owned()))
            .with_body(json!({"value": [{"@iot.id": 1, "phenomenonTime": "soon"}, {"@iot.id": 2, "phenomenonTime": "2026-11-12T09:00:00Z"}]}).to_string())
            .create();

        let (written, positions, _) = read(&server, &incremental(""), Positions::default());

        assert_eq!(written.unwrap(), [json!(1), json!(2)]);
        assert_eq!(positions.get(&id(5)).unwrap().seen(), &seen(&[(2, "09:00")]));
    }

    #[test]
    fn an_incremental_read_without_its_cursor_fails_before_any_request() {
        let mut server = mockito::Server::new();
        let untouched = server.mock("GET", Matcher::Any).expect(0).create();

        let (result, payloads) = collect(collector(&server, &incremental(""), RequestHeaders::default(), &RUNNING));

        untouched.assert();
        assert!(payloads.is_empty());
        let error = result.unwrap_err();
        assert!(
            matches!(&error, CollectorError::CursorWithheld { root: withheld } if **withheld == root(&server)),
            "{error:?}"
        );
    }
}

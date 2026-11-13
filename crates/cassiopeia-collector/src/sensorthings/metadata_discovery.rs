use crate::{
    downloader::Downloader,
    error::CollectorError,
    sensorthings::{
        page_reader::PageReader,
        traversal::{TraversalEnd, traverse},
    },
};
use cassiopeia_sensorthings::{request::discovery_request::metadata_discovery_request, service::SensorThingsService};
use serde_json::{Map, Value};
use std::{ops::ControlFlow, sync::atomic::AtomicBool};

/// Reads every Datastream `service` selects, with its Thing, the Thing's Locations and its
/// `ObservedProperty`, and hands each to `datastream` in the order the server sent them.
///
/// The metadata discovery request is followed as a run follows its requests: through `downloader`,
/// with the input's page size, page limit and delay, each next link kept on the service root's
/// origin, and stopping once `shutdown` is set. A limit that stops the read is returned rather than
/// failing it, so the caller decides what a partial read means.
///
/// # Errors
///
/// Returns, as `E`, every [`CollectorError`] a run's read fails with: a stop, a link loop, a next
/// link that is not followed, a failed request and a page that is no collection; and every error
/// `datastream` returns, which ends the read before its next request.
pub fn discover_metadata<E: From<CollectorError>>(
    service: &SensorThingsService,
    downloader: Downloader,
    shutdown: &'static AtomicBool,
    mut datastream: impl FnMut(Map<String, Value>) -> Result<(), E>,
) -> Result<TraversalEnd, E> {
    let settings = service.settings();
    // The reader names the root in its failures, while the service keeps its own.
    let mut reader = PageReader::new(service.root().clone(), downloader, settings.delay(), shutdown);
    traverse(
        &mut reader,
        metadata_discovery_request(service.root(), settings),
        settings.limits().pages(),
        |served| datastream(served).map(|()| ControlFlow::Continue(())),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        downloader::Downloader,
        error::CollectorError,
        sensorthings::{metadata_discovery::discover_metadata, traversal::TraversalEnd},
    };
    use cassiopeia_common::{request_header::RequestHeaders, user_agent::UserAgent};
    use cassiopeia_sensorthings::{service::SensorThingsService, service_root::ServiceRoot};
    use mockito::ServerGuard;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use std::sync::atomic::AtomicBool;
    use thiserror::Error;
    use url::Url;

    /// The path and query of the metadata discovery request with the default settings.
    const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=1000";

    /// A shutdown flag never set, as in a run nobody stops.
    static RUNNING: AtomicBool = AtomicBool::new(false);

    /// What a caller of the discovery fails with: the read's own failure, or its refusal of an item.
    #[derive(Debug, Error)]
    enum DiscoveryFailure {
        #[error(transparent)]
        Collector(#[from] CollectorError),
        #[error("data stream {0} is refused")]
        Refused(Value),
    }

    fn service(server: &ServerGuard, settings: &str) -> SensorThingsService {
        let root = ServiceRoot::try_from(Url::parse(&format!("{}/v1.1", server.url())).unwrap()).unwrap();
        SensorThingsService::new(root, serde_json::from_str(settings).unwrap())
    }

    fn downloader() -> Downloader {
        Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING)
    }

    /// A page body holding Datastreams `ids` and, when given, the next link `next`.
    fn page(ids: &[u64], next: Option<&str>) -> String {
        let mut body = json!({"value": ids.iter().map(|id| json!({"@iot.id": id})).collect::<Vec<_>>()});
        if let Some(next) = next {
            body["@iot.nextLink"] = json!(next);
        }
        body.to_string()
    }

    #[test]
    fn metadata_discovery_follows_every_page_in_order() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Datastreams?$skip=2", server.url());
        let first_page = server.mock("GET", METADATA).with_body(page(&[1, 2], Some(&second))).expect(1).create();
        let second_page = server.mock("GET", "/v1.1/Datastreams?$skip=2").with_body(page(&[3], None)).expect(1).create();
        let mut ids = Vec::new();

        let end = discover_metadata(&service(&server, "{}"), downloader(), &RUNNING, |datastream: Map<String, Value>| {
            ids.push(datastream["@iot.id"].clone());
            Ok::<(), CollectorError>(())
        })
        .unwrap();

        first_page.assert();
        second_page.assert();
        assert_eq!(end, TraversalEnd::LastPage);
        assert_eq!(ids, [json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn the_discovery_selects_and_pages_as_the_service_settings_say() {
        let mut server = mockito::Server::new();
        let discovery = server
            .mock(
                "GET",
                "/v1.1/Datastreams?$filter=startswith(name,%27ILM_I68-%27)&$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=10",
            )
            .with_body(page(&[1], None))
            .expect(1)
            .create();

        let end = discover_metadata(
            &service(&server, r#"{"datastreams": "startswith(name,'ILM_I68-')", "pageSize": 10}"#),
            downloader(),
            &RUNNING,
            |_| Ok::<(), CollectorError>(()),
        )
        .unwrap();

        discovery.assert();
        assert_eq!(end, TraversalEnd::LastPage);
    }

    #[test]
    fn a_refused_data_stream_ends_the_discovery_before_its_next_request() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Datastreams?$skip=1", server.url());
        server.mock("GET", METADATA).with_body(page(&[1], Some(&second))).expect(1).create();
        let untouched = server.mock("GET", "/v1.1/Datastreams?$skip=1").expect(0).create();

        let failure = discover_metadata(&service(&server, "{}"), downloader(), &RUNNING, |datastream| {
            Err(DiscoveryFailure::Refused(datastream["@iot.id"].clone()))
        })
        .unwrap_err();

        untouched.assert();
        assert!(matches!(&failure, DiscoveryFailure::Refused(id) if *id == json!(1)), "{failure:?}");
    }

    #[test]
    fn a_failed_read_reaches_the_caller_as_its_own_failure() {
        let mut server = mockito::Server::new();
        server.mock("GET", METADATA).with_body("<html>").expect(1).create();

        let failure = discover_metadata(&service(&server, "{}"), downloader(), &RUNNING, |_| Ok::<(), DiscoveryFailure>(())).unwrap_err();

        assert!(
            matches!(&failure, DiscoveryFailure::Collector(CollectorError::PageUnreadable { .. })),
            "{failure:?}"
        );
    }
}

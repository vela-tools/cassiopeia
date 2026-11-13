use crate::error::{CliError, Result};
use cassiopeia_collector::{
    downloader::Downloader,
    sensorthings::{limit_reached::LimitReached, metadata_discovery::discover_metadata, traversal::TraversalEnd},
};
use cassiopeia_sensorthings::{
    offered_readings::{offered_reading::OfferedReading, reading_catalog::ReadingCatalog},
    service::SensorThingsService,
};
use getset::Getters;
use std::sync::atomic::AtomicBool;

/// The readings a SensorThings server offers, as `cassiopeia sensorthings readings` lists them.
#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct ReadingsListing {
    /// The readings, sorted by name.
    readings: Vec<OfferedReading>,
    /// The collection limit that stopped the read before the server's last page, when one did, so
    /// the readings cover only the data streams read before it.
    limit_reached: Option<LimitReached>,
}

/// Lists the readings `service` offers, reading its metadata discovery through `downloader` as a run
/// reads a server, and stopping once `shutdown` is set.
///
/// # Errors
///
/// Returns [`CliError::SensorThingsRead`] for every failure a run's read of the server fails with,
/// and [`CliError::OfferedReading`] for a Datastream that names no reading.
pub fn list_readings(service: &SensorThingsService, downloader: Downloader, shutdown: &'static AtomicBool) -> Result<ReadingsListing> {
    let mut catalog = ReadingCatalog::default();
    let end = discover_metadata(service, downloader, shutdown, |datastream| catalog.add(&datastream).map_err(CliError::from))?;
    let limit_reached = match end {
        TraversalEnd::LastPage => None,
        TraversalEnd::LimitReached(limit) => Some(limit),
    };
    Ok(ReadingsListing {
        readings: catalog.into_readings(),
        limit_reached,
    })
}

#[cfg(test)]
mod tests {
    use crate::{error::CliError, sensorthings_readings::list_readings};
    use cassiopeia_collector::{downloader::Downloader, error::CollectorError};
    use cassiopeia_common::{request_header::RequestHeaders, user_agent::UserAgent};
    use cassiopeia_sensorthings::{entity_id::EntityId, offered_readings::error::OfferedReadingError, service::SensorThingsService, service_root::ServiceRoot};
    use mockito::ServerGuard;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;

    /// The path and query of the metadata discovery request with the default settings.
    const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty&$orderby=id%20asc&$top=1000";

    /// A shutdown flag never set, as in a listing nobody stops.
    static RUNNING: AtomicBool = AtomicBool::new(false);

    fn service(server: &ServerGuard, settings: &str) -> SensorThingsService {
        let root: ServiceRoot = format!("{}/v1.1", server.url()).parse().unwrap();
        SensorThingsService::new(root, serde_json::from_str(settings).unwrap())
    }

    fn downloader() -> Downloader {
        Downloader::new(UserAgent::from("cassiopeia/test".to_owned()), RequestHeaders::default(), &RUNNING)
    }

    /// A served Datastream `id` of Thing 1, whose readings are named `pm10`.
    fn pm10(id: u64) -> serde_json::Value {
        json!({"@iot.id": id, "unitOfMeasurement": {"symbol": "ug/m3"}, "Thing": {"@iot.id": 1}, "ObservedProperty": {"name": "pm10"}})
    }

    #[test]
    fn a_listing_cut_short_by_the_page_limit_names_the_limit_and_keeps_what_it_read() {
        let mut server = mockito::Server::new();
        let second = format!("{}/v1.1/Datastreams?$skip=1", server.url());
        server
            .mock("GET", METADATA)
            .with_body(json!({"value": [pm10(1)], "@iot.nextLink": second}).to_string())
            .expect(1)
            .create();

        let listing = list_readings(&service(&server, r#"{"limits": {"pages": 1}}"#), downloader(), &RUNNING).unwrap();

        assert_eq!(listing.readings().len(), 1);
        assert_eq!(listing.readings()[0].datastreams(), 1);
        assert_eq!(listing.limit_reached().as_ref().map(|limit| limit.next().as_str()), Some(second.as_str()));
    }

    #[test]
    fn a_data_stream_without_a_named_observed_property_fails_the_listing() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", METADATA)
            .with_body(json!({"value": [{"@iot.id": 9, "Thing": {"@iot.id": 1}, "ObservedProperty": {}}]}).to_string())
            .expect(1)
            .create();

        let error = list_readings(&service(&server, "{}"), downloader(), &RUNNING).unwrap_err();

        assert!(
            matches!(&error, CliError::OfferedReading(OfferedReadingError::ObservedPropertyUnnamed { datastream }) if *datastream == EntityId::Integer(9)),
            "{error:?}"
        );
    }

    #[test]
    fn a_failed_request_fails_the_listing_as_it_fails_a_run() {
        let mut server = mockito::Server::new();
        server.mock("GET", METADATA).with_status(404).expect(1).create();

        let error = list_readings(&service(&server, "{}"), downloader(), &RUNNING).unwrap_err();

        assert!(matches!(&error, CliError::SensorThingsRead(CollectorError::HttpStatus { .. })), "{error:?}");
        assert!(error.to_string().ends_with("answered 404 Not Found"), "{error}");
    }
}

use crate::error::{CliError, Result};
use cassiopeia_collector::{
    downloader::Downloader,
    sensorthings::{limit_reached::LimitReached, metadata_discovery::discover_metadata},
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
    /// The collection limits that stopped a discovery before the server's last page, so the
    /// readings cover only the data streams read before them.
    limits_reached: Vec<LimitReached>,
}

/// Lists the readings `service` offers, those of its Datastreams and, when its settings select
/// `MultiDatastreams` and the service root lists them, those of the elements of its
/// `MultiDatastreams`, reading its metadata discoveries through `downloader` as a run reads a
/// server, and stopping once `shutdown` is set.
///
/// # Errors
///
/// Returns [`CliError::SensorThingsRead`] for every failure a run's read of the server fails with,
/// and [`CliError::OfferedReading`] for a data stream that names no reading.
pub fn list_readings(service: &SensorThingsService, downloader: Downloader, shutdown: &'static AtomicBool) -> Result<ReadingsListing> {
    let mut catalog = ReadingCatalog::default();
    let limits_reached = discover_metadata(service, downloader, shutdown, |kind, datastream| {
        catalog.add(kind, datastream).map_err(CliError::from)
    })?;
    Ok(ReadingsListing {
        readings: catalog.into_readings(),
        limits_reached,
    })
}

#[cfg(test)]
mod tests {
    use crate::{error::CliError, sensorthings_readings::list_readings};
    use cassiopeia_collector::{downloader::Downloader, error::CollectorError};
    use cassiopeia_common::{request_header::RequestHeaders, user_agent::UserAgent};
    use cassiopeia_sensorthings::{
        entity_id::EntityId,
        offered_readings::error::OfferedReadingError,
        service::SensorThingsService,
        service_root::ServiceRoot,
        stream_id::StreamId,
    };
    use mockito::ServerGuard;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;

    /// The path and query of the metadata discovery request with the default settings.
    const METADATA: &str = "/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000";

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
        assert_eq!(
            listing.limits_reached().iter().map(|limit| limit.next().as_str()).collect::<Vec<_>>(),
            [second.as_str()]
        );
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
            matches!(&error, CliError::OfferedReading(OfferedReadingError::ObservedPropertyUnnamed { datastream }) if *datastream == StreamId::Datastream(EntityId::Integer(9))),
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

    #[test]
    fn the_elements_of_multi_datastreams_a_root_lists_are_listed_with_their_units() {
        let mut server = mockito::Server::new();
        let mocks = [
            server
                .mock("GET", METADATA)
                .with_body(json!({"value": [pm10(1)]}).to_string())
                .expect(1)
                .create(),
            server
                .mock("GET", "/v1.1")
                .with_body(json!({"value": [{"name": "Datastreams"}, {"name": "MultiDatastreams"}]}).to_string())
                .expect(1)
                .create(),
            server
                .mock(
                    "GET",
                    "/v1.1/MultiDatastreams?$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc&$top=1000",
                )
                .with_body(
                    json!({"value": [{
                        "@iot.id": 4,
                        "Thing": {"@iot.id": 1},
                        "ObservedProperties": [{"name": "windSpeed"}, {"name": "windDirection"}],
                        "unitOfMeasurements": [{"symbol": "m/s"}, {"symbol": "deg"}],
                        "multiObservationDataTypes": ["OM_Measurement", "OM_Measurement"],
                    }]})
                    .to_string(),
                )
                .expect(1)
                .create(),
        ];

        let listing = list_readings(&service(&server, r#"{"multiDatastreams": true}"#), downloader(), &RUNNING).unwrap();

        mocks.iter().for_each(mockito::Mock::assert);
        let rows: Vec<(String, Vec<String>, usize, usize)> = listing
            .readings()
            .iter()
            .map(|reading| {
                (
                    reading.name().to_string(),
                    reading.units().iter().map(ToString::to_string).collect(),
                    reading.datastreams(),
                    reading.multi_datastreams(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("pm10".to_owned(), vec!["ug/m3".to_owned()], 1, 0),
                ("windDirection".to_owned(), vec!["deg".to_owned()], 0, 1),
                ("windSpeed".to_owned(), vec!["m/s".to_owned()], 0, 1),
            ]
        );
    }
}

use crate::{
    request::{query_value::query_value, resource_request::resource_request},
    service_root::ServiceRoot,
    settings::{features_of_interest::FeaturesOfInterest, sensorthings_settings::SensorThingsSettings},
};
use url::Url;

/// What a metadata discovery expands on every Datastream: its Thing with the Thing's Locations, its
/// `ObservedProperty`, and its `Sensor`.
///
/// The Locations are expanded by path, `Thing/Locations`, rather than nested as
/// `Thing($expand=Locations)`: OGC 18-088 section 9.3.2.1 defines both, and RIVM Samen Meten ignores
/// an `$expand` nested in parentheses while it honours query options one level deep, as on
/// `Observations`. A Datastream links exactly one `Sensor` (section 8.2.4), so expanding it here
/// reads each one once per data stream rather than once per Observation.
const METADATA_EXPAND: &str = "Thing/Locations,ObservedProperty,Sensor";

/// What a latest discovery expands on every Datastream: what a metadata discovery expands, and its
/// newest Observation.
const LATEST_EXPAND: &str = "Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)";

/// What a latest discovery that requests features of interest expands on every Datastream: what
/// [`LATEST_EXPAND`] expands, with the newest Observation's `FeatureOfInterest` expanded inside it, as
/// OGC 18-088 section 9.3.2.1 allows. A server that ignores an `$expand` nested in parentheses, as
/// RIVM Samen Meten does, serves the Observation without it.
const LATEST_EXPAND_WITH_FEATURES: &str =
    "Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)";

/// The order every discovery pages in, so a `$skip`-built page follows the one before it.
const BY_ID: &str = "id%20asc";

/// The request that lists every selected Datastream with its Thing, the Thing's Locations, its
/// `ObservedProperty` and its `Sensor`, without Observations, in id order, `pageSize` a page.
///
/// Query parameters are written as in [`latest_discovery_request`].
#[must_use]
pub fn metadata_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings) -> Url {
    discovery_request(root, settings, METADATA_EXPAND)
}

/// The request a latest read starts with, and the only one it sends: every selected Datastream with
/// its Thing, the Thing's Locations, its `ObservedProperty`, its `Sensor` and its newest Observation,
/// with that Observation's `FeatureOfInterest` when the settings request features of interest, in id
/// order, `pageSize` a page.
///
/// Query parameters are written in the order `$filter`, `$expand`, `$orderby`, `$top`, and `$filter`
/// only when the settings select data streams.
#[must_use]
pub fn latest_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings) -> Url {
    let expand = match settings.features_of_interest() {
        FeaturesOfInterest::Requested => LATEST_EXPAND_WITH_FEATURES,
        FeaturesOfInterest::Omitted => LATEST_EXPAND,
    };
    discovery_request(root, settings, expand)
}

/// The `Datastreams` request of `root` that selects as `settings` say and expands `expand`.
fn discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, expand: &str) -> Url {
    let filter = settings
        .datastreams()
        .as_ref()
        .map(|filter| format!("$filter={}&", query_value(filter.as_str())))
        .unwrap_or_default();
    resource_request(
        root,
        "Datastreams",
        &format!("{filter}$expand={expand}&$orderby={BY_ID}&$top={}", settings.page_size()),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        request::discovery_request::{latest_discovery_request, metadata_discovery_request},
        service_root::ServiceRoot,
        settings::sensorthings_settings::SensorThingsSettings,
    };
    use pretty_assertions::assert_eq;
    use url::Url;

    fn root(url: &str) -> ServiceRoot {
        ServiceRoot::try_from(Url::parse(url).unwrap()).unwrap()
    }

    fn settings(json: &str) -> SensorThingsSettings {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn latest_discovery_request_is_built_exactly() {
        let request = latest_discovery_request(&root("https://h.example/v1.1"), &settings("{}"));

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn a_filter_leads_the_discovery_query() {
        let request = latest_discovery_request(
            &root("https://h.example/v1.1"),
            &settings(r#"{"datastreams": "ObservedProperty/name eq 'pm10'"}"#),
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn the_request_is_built_under_a_root_with_a_path_and_a_port() {
        let request = latest_discovery_request(&root("http://localhost:18080/FROST-Server/v1.1/"), &settings(r#"{"pageSize": 10}"#));

        assert_eq!(
            request.as_str(),
            "http://localhost:18080/FROST-Server/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=10"
        );
    }

    #[test]
    fn requested_features_of_interest_are_expanded_inside_the_newest_observation() {
        let request = latest_discovery_request(
            &root("https://h.example/v1.1"),
            &settings(r#"{"datastreams": "startswith(name,'bus-')", "featuresOfInterest": true}"#),
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$filter=startswith(name,%27bus-%27)&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn the_metadata_discovery_never_expands_features_of_interest() {
        let request = metadata_discovery_request(&root("https://h.example/v1.1"), &settings(r#"{"featuresOfInterest": true}"#));

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn metadata_discovery_request_is_built_exactly() {
        let request = metadata_discovery_request(&root("https://h.example/v1.1"), &settings("{}"));

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn a_filter_leads_the_metadata_discovery_query() {
        let request = metadata_discovery_request(
            &root("https://h.example/v1.1"),
            &settings(r#"{"datastreams": "ObservedProperty/name eq 'pm10'", "pageSize": 200}"#),
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=200"
        );
    }
}

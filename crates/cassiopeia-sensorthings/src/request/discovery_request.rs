use crate::{
    request::{query_value::query_value, resource_request::resource_request},
    service_root::ServiceRoot,
    settings::{features_of_interest::FeaturesOfInterest, sensorthings_settings::SensorThingsSettings},
    stream_kind::StreamKind,
};
use url::Url;

/// What a latest discovery expands on every data stream, besides what a metadata discovery expands:
/// its newest Observation.
const NEWEST_OBSERVATION: &str = "Observations($orderby=phenomenonTime%20desc;$top=1)";

/// What a latest discovery that requests features of interest expands on every data stream, besides
/// what a metadata discovery expands: its newest Observation with that Observation's
/// `FeatureOfInterest` expanded inside it, as OGC 18-088 section 9.3.2.1 allows. A server that
/// ignores an `$expand` nested in parentheses, as RIVM Samen Meten does, serves the Observation
/// without it.
const NEWEST_OBSERVATION_WITH_FEATURE: &str = "Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)";

/// The order every discovery pages in, so a `$skip`-built page follows the one before it.
const BY_ID: &str = "id%20asc";

/// The request that lists every selected data stream of `kind` with its Thing, the Thing's
/// Locations, what it observes and its `Sensor`, without Observations, in id order, `pageSize` a
/// page.
///
/// Query parameters are written as in [`latest_discovery_request`].
#[must_use]
pub fn metadata_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, kind: StreamKind) -> Url {
    discovery_request(root, settings, kind, metadata_expand(kind))
}

/// The request a latest read starts with for the data streams of `kind`, and the only one it sends
/// for them: every selected data stream of `kind` with its Thing, the Thing's Locations, what it
/// observes, its `Sensor` and its newest Observation, with that Observation's `FeatureOfInterest`
/// when the settings request features of interest, in id order, `pageSize` a page.
///
/// Query parameters are written in the order `$filter`, `$expand`, `$orderby`, `$top`, and `$filter`
/// only when the settings select data streams of `kind` by an expression.
#[must_use]
pub fn latest_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, kind: StreamKind) -> Url {
    let newest = match settings.features_of_interest() {
        FeaturesOfInterest::Requested => NEWEST_OBSERVATION_WITH_FEATURE,
        FeaturesOfInterest::Omitted => NEWEST_OBSERVATION,
    };
    discovery_request(root, settings, kind, &format!("{},{newest}", metadata_expand(kind)))
}

/// What a metadata discovery expands on every data stream of `kind`: its Thing with the Thing's
/// Locations, what it observes, and its `Sensor`.
///
/// A Datastream observes one `ObservedProperty` (OGC 18-088 section 8.2.4), and a `MultiDatastream`
/// observes its `ObservedProperties`, in the order of the values of its results (section 12). The
/// Locations are expanded by path, `Thing/Locations`, rather than nested as
/// `Thing($expand=Locations)`: section 9.3.2.1 defines both, and RIVM Samen Meten ignores an
/// `$expand` nested in parentheses while it honours query options one level deep, as on
/// `Observations`. A data stream links exactly one `Sensor`, so expanding it here reads each one once
/// per data stream rather than once per Observation.
const fn metadata_expand(kind: StreamKind) -> &'static str {
    match kind {
        StreamKind::Datastream => "Thing/Locations,ObservedProperty,Sensor",
        StreamKind::MultiDatastream => "Thing/Locations,ObservedProperties,Sensor",
    }
}

/// The request of the collection of `kind` under `root` that selects as `settings` say and expands
/// `expand`.
fn discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, kind: StreamKind, expand: &str) -> Url {
    let filter = settings
        .filter(kind)
        .map(|filter| format!("$filter={}&", query_value(filter.as_str())))
        .unwrap_or_default();
    resource_request(
        root,
        kind.collection(),
        &format!("{filter}$expand={expand}&$orderby={BY_ID}&$top={}", settings.page_size()),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        request::discovery_request::{latest_discovery_request, metadata_discovery_request},
        service_root::ServiceRoot,
        settings::sensorthings_settings::SensorThingsSettings,
        stream_kind::StreamKind,
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
        let request = latest_discovery_request(&root("https://h.example/v1.1"), &settings("{}"), StreamKind::Datastream);

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
            StreamKind::Datastream,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn the_request_is_built_under_a_root_with_a_path_and_a_port() {
        let request = latest_discovery_request(
            &root("http://localhost:18080/FROST-Server/v1.1/"),
            &settings(r#"{"pageSize": 10}"#),
            StreamKind::Datastream,
        );

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
            StreamKind::Datastream,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$filter=startswith(name,%27bus-%27)&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn the_metadata_discovery_never_expands_features_of_interest() {
        let request = metadata_discovery_request(
            &root("https://h.example/v1.1"),
            &settings(r#"{"featuresOfInterest": true}"#),
            StreamKind::Datastream,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn metadata_discovery_request_is_built_exactly() {
        let request = metadata_discovery_request(&root("https://h.example/v1.1"), &settings("{}"), StreamKind::Datastream);

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
            StreamKind::Datastream,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=200"
        );
    }

    #[test]
    fn every_multi_datastream_is_discovered_with_its_observed_properties() {
        let every = settings(r#"{"multiDatastreams": true}"#);

        assert_eq!(
            latest_discovery_request(&root("https://h.example/v1.1"), &every, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000"
        );
        assert_eq!(
            metadata_discovery_request(&root("https://h.example/v1.1"), &every, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn a_multi_datastream_filter_leads_the_multi_datastream_queries_only() {
        let filtered = settings(r#"{"datastreams": "ObservedProperty/name eq 'pm10'", "multiDatastreams": "startswith(name,'ws-')"}"#);

        assert_eq!(
            latest_discovery_request(&root("https://h.example/v1.1"), &filtered, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$filter=startswith(name,%27ws-%27)&$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc&$top=1000"
        );
        assert_eq!(
            metadata_discovery_request(&root("https://h.example/v1.1"), &filtered, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$filter=startswith(name,%27ws-%27)&$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc&$top=1000"
        );
        assert_eq!(
            metadata_discovery_request(&root("https://h.example/v1.1"), &filtered, StreamKind::Datastream).as_str(),
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc&$top=1000"
        );
    }

    #[test]
    fn a_latest_multi_datastream_discovery_expands_requested_features_of_interest() {
        let request = latest_discovery_request(
            &root("https://h.example/v1.1"),
            &settings(r#"{"multiDatastreams": true, "featuresOfInterest": true}"#),
            StreamKind::MultiDatastream,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/MultiDatastreams?$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc&$top=1000"
        );
    }
}

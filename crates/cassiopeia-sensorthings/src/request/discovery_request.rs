use crate::{
    request::{query_value::query_value, resource_request::resource_request},
    service_root::ServiceRoot,
    settings::{
        entity_filter::EntityFilter,
        features_of_interest::FeaturesOfInterest,
        page_size::PageSize,
        sensorthings_settings::SensorThingsSettings,
        things::Things,
    },
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

/// The collection a server serves its Things in (OGC 18-088 section 9.2.1).
const THINGS: &str = "Things";

/// What a Things discovery expands on every Thing: its Locations, which a record's `location` and
/// `locations` are read from (OGC 18-088 section 8.2.2).
const LOCATIONS: &str = "Locations";

/// The request that lists every selected data stream of `kind` with its Thing, the Thing's
/// Locations, what it observes and its `Sensor`, without Observations, in id order, `pageSize` a
/// page.
///
/// Query parameters are written as in [`latest_discovery_request`].
#[must_use]
pub fn metadata_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, kind: StreamKind) -> Url {
    stream_discovery_request(root, settings, kind, metadata_expand(kind))
}

/// The request a latest read starts with for the data streams of `kind`, and the only one it sends
/// for them: every selected data stream of `kind` with its Thing, the Thing's Locations, what it
/// observes, its `Sensor` and its newest Observation, with that Observation's `FeatureOfInterest`
/// when the settings request features of interest, in id order, `pageSize` a page.
///
/// Query parameters are written in the order `$filter`, `$top`, `$expand`, `$orderby`, and `$filter`
/// only when the settings select data streams of `kind` by an expression. A server evaluates query
/// options in one order whatever order they are written in (OGC 18-088 section 9.3.1, Req 22), but
/// `$top` comes before the `$top=1` of the newest Observation's expansion: istSOS4 1.3 builds its next link from the first `$top=` of the
/// request, so a later `$top` gives it a page of one data stream, and every page then repeats all but
/// one of the data streams of the page before it.
#[must_use]
pub fn latest_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, kind: StreamKind) -> Url {
    let newest = match settings.features_of_interest() {
        FeaturesOfInterest::Requested => NEWEST_OBSERVATION_WITH_FEATURE,
        FeaturesOfInterest::Omitted => NEWEST_OBSERVATION,
    };
    stream_discovery_request(root, settings, kind, &format!("{},{newest}", metadata_expand(kind)))
}

/// The request a latest read that lists Things sends after its data stream discoveries: every Thing
/// `things` selects with its Locations, in id order, `pageSize` a page, or `None` when the settings
/// list no Thing beyond those of the data streams read.
///
/// A Thing may have no data stream (OGC 18-088 section 8.2.1, Table 4), so a read that finds Things
/// through their data streams never finds it. Query parameters are written as in
/// [`latest_discovery_request`], and `$filter` only when `things` selects Things by an expression.
#[must_use]
pub fn things_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings) -> Option<Url> {
    let filter = match settings.things() {
        Things::OfDatastreams => return None,
        Things::Every => None,
        Things::Filtered(filter) => Some(filter),
    };
    Some(discovery_request(root, THINGS, filter, LOCATIONS, settings.page_size()))
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
fn stream_discovery_request(root: &ServiceRoot, settings: &SensorThingsSettings, kind: StreamKind, expand: &str) -> Url {
    discovery_request(root, kind.collection(), settings.filter(kind), expand, settings.page_size())
}

/// The request of the collection `collection` under `root` that selects the entities `filter`
/// matches, every one when it is `None`, and expands `expand`, in id order, `page_size` a page.
fn discovery_request(root: &ServiceRoot, collection: &str, filter: Option<&EntityFilter>, expand: &str, page_size: PageSize) -> Url {
    let filter = filter.map(|filter| format!("$filter={}&", query_value(filter.as_str()))).unwrap_or_default();
    resource_request(root, collection, &format!("{filter}$top={page_size}&$expand={expand}&$orderby={BY_ID}"))
}

#[cfg(test)]
mod tests {
    use crate::{
        request::discovery_request::{latest_discovery_request, metadata_discovery_request, things_discovery_request},
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
            "https://h.example/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc"
        );
    }

    #[test]
    fn the_page_size_comes_before_the_top_of_the_newest_observation() {
        let request = latest_discovery_request(
            &root("https://h.example/v1.1"),
            &settings(r#"{"datastreams": "endswith(name,'-pm10')", "pageSize": 25, "featuresOfInterest": true}"#),
            StreamKind::Datastream,
        );
        let query = request.query().unwrap_or_default();

        assert_eq!(query.find("$top=").and_then(|start| query.get(start..start + 7)), Some("$top=25"));
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
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc"
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
            "http://localhost:18080/FROST-Server/v1.1/Datastreams?$top=10&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc"
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
            "https://h.example/v1.1/Datastreams?$filter=startswith(name,%27bus-%27)&$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc"
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
            "https://h.example/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc"
        );
    }

    #[test]
    fn metadata_discovery_request_is_built_exactly() {
        let request = metadata_discovery_request(&root("https://h.example/v1.1"), &settings("{}"), StreamKind::Datastream);

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams?$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc"
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
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$top=200&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc"
        );
    }

    #[test]
    fn every_multi_datastream_is_discovered_with_its_observed_properties() {
        let every = settings(r#"{"multiDatastreams": true}"#);

        assert_eq!(
            latest_discovery_request(&root("https://h.example/v1.1"), &every, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc"
        );
        assert_eq!(
            metadata_discovery_request(&root("https://h.example/v1.1"), &every, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc"
        );
    }

    #[test]
    fn a_multi_datastream_filter_leads_the_multi_datastream_queries_only() {
        let filtered = settings(r#"{"datastreams": "ObservedProperty/name eq 'pm10'", "multiDatastreams": "startswith(name,'ws-')"}"#);

        assert_eq!(
            latest_discovery_request(&root("https://h.example/v1.1"), &filtered, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$filter=startswith(name,%27ws-%27)&$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1)&$orderby=id%20asc"
        );
        assert_eq!(
            metadata_discovery_request(&root("https://h.example/v1.1"), &filtered, StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$filter=startswith(name,%27ws-%27)&$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor&$orderby=id%20asc"
        );
        assert_eq!(
            metadata_discovery_request(&root("https://h.example/v1.1"), &filtered, StreamKind::Datastream).as_str(),
            "https://h.example/v1.1/Datastreams?$filter=ObservedProperty/name%20eq%20%27pm10%27&$top=1000&$expand=Thing/Locations,ObservedProperty,Sensor&$orderby=id%20asc"
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
            "https://h.example/v1.1/MultiDatastreams?$top=1000&$expand=Thing/Locations,ObservedProperties,Sensor,Observations($orderby=phenomenonTime%20desc;$top=1;$expand=FeatureOfInterest)&$orderby=id%20asc"
        );
    }

    #[test]
    fn every_thing_is_discovered_with_its_locations() {
        let request = things_discovery_request(&root("https://h.example/v1.1"), &settings(r#"{"things": true}"#));

        assert_eq!(
            request.map(String::from).as_deref(),
            Some("https://h.example/v1.1/Things?$top=1000&$expand=Locations&$orderby=id%20asc")
        );
    }

    #[test]
    fn a_things_filter_leads_the_things_query() {
        let request = things_discovery_request(&root("https://h.example/v1.1"), &settings(r#"{"things": "startswith(name,'ILM_')"}"#));

        assert_eq!(
            request.map(String::from).as_deref(),
            Some("https://h.example/v1.1/Things?$filter=startswith(name,%27ILM_%27)&$top=1000&$expand=Locations&$orderby=id%20asc")
        );
    }

    #[test]
    fn the_things_discovery_pages_as_every_discovery_and_ignores_the_data_stream_filters() {
        let request = things_discovery_request(
            &root("http://localhost:18080/FROST-Server/v1.1/"),
            &settings(r#"{"things": true, "datastreams": "endswith(name,'-pm10')", "multiDatastreams": true, "pageSize": 2}"#),
        );

        assert_eq!(
            request.map(String::from).as_deref(),
            Some("http://localhost:18080/FROST-Server/v1.1/Things?$top=2&$expand=Locations&$orderby=id%20asc")
        );
    }

    #[test]
    fn no_thing_is_discovered_unless_the_settings_list_things() {
        for json in ["{}", r#"{"things": false}"#] {
            assert_eq!(things_discovery_request(&root("https://h.example/v1.1"), &settings(json)), None, "{json}");
        }
    }
}

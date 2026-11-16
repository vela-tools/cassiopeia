use crate::{
    encoding_type::ENCODING_TYPE_MEMBER,
    feature_of_interest_members::{CARRIED_FEATURE_OF_INTEREST_MEMBERS, FEATURE_MEMBER, FEATURE_OF_INTEREST_MEMBER},
    instant_text::instant_text,
    observation_members::{CARRIED_OBSERVATION_MEMBERS, RESULT_MEMBER},
    phenomenon_time::PHENOMENON_TIME_MEMBER,
    read_window::ReadWindow,
    request::{entity_key::entity_key, query_value::query_value, resource_request::resource_request},
    service_root::ServiceRoot,
    settings::{features_of_interest::FeaturesOfInterest, page_size::PageSize},
    stream_id::StreamId,
    version::StaVersion,
};
use url::Url;

/// The order Observations are paged in: by `phenomenonTime`, then by id, so Observations of one
/// time keep one order across pages (OGC 18-088 section 9.3.3.1, Req 25).
const BY_TIME_THEN_ID: &str = "phenomenonTime%20asc,id%20asc";

/// The selection clause of an entity's id, which the server serves as `@iot.id` (OGC 18-088 section
/// 9.3.2.2, Example 19).
const ID_SELECTION: &str = "id";

/// The request for the Observations of the data stream `datastream` whose `phenomenonTime` falls in
/// `window`, oldest first, `page_size` a page, each with its `FeatureOfInterest` when `features` asks
/// for it: `Datastreams({id})/Observations` for a Datastream, and `MultiDatastreams({id})/Observations`
/// for a `MultiDatastream`.
///
/// The bounds are written as RFC 3339 instants in UTC with a `Z`, with a fraction only when they have
/// one. Query parameters are written in the order `$filter`, `$orderby`, `$expand`, `$select`,
/// `$top`, and `$expand` only when features of interest are requested. OGC 18-088 section 9.3.3.5
/// does not define how a server compares a `phenomenonTime` interval with an instant.
///
/// `$select` names the Observation members a record reads and no other (section 9.3.2.2), so the
/// server leaves out the links it serves with every Observation by default, which make up most of a
/// page. The selection names the id and the `phenomenonTime` the request orders by: some servers,
/// RIVM Samen Meten among them, fail a request that orders by a property it does not select.
#[must_use]
pub fn observations_request(root: &ServiceRoot, datastream: &StreamId, window: ReadWindow, page_size: PageSize, features: FeaturesOfInterest) -> Url {
    let filter = format!(
        "phenomenonTime ge {} and phenomenonTime lt {}",
        instant_text(window.from()),
        instant_text(window.to())
    );
    let expand = match features {
        FeaturesOfInterest::Requested => format!("$expand={}&", feature_of_interest_expansion(root.version())),
        FeaturesOfInterest::Omitted => String::new(),
    };
    let select = format!(
        "{ID_SELECTION},{PHENOMENON_TIME_MEMBER},{RESULT_MEMBER},{}",
        CARRIED_OBSERVATION_MEMBERS.join(",")
    );
    resource_request(
        root,
        &format!("{}{}/Observations", datastream.kind().collection(), entity_key(datastream.id())),
        &format!(
            "$filter={}&$orderby={BY_TIME_THEN_ID}&{expand}$select={select}&$top={page_size}",
            query_value(&filter)
        ),
    )
}

/// The `$expand` item that serves each Observation with the `FeatureOfInterest` it was made at (OGC
/// 18-088 section 9.3.2.1), from a server of `version`.
///
/// An expanded entity is served whole, however the Observation is selected (section 9.3.2.2, Example
/// 19), and some servers, FROST-Server among them, then serve it with its own link. From a version
/// 1.1 server it is therefore selected in turn, with a `$select` in parentheses: its id and the
/// members a reading's `featureOfInterest` reads. `properties` is a member of a `FeatureOfInterest`
/// from version 1.1 on (section 8.2.8, Table 20), and a version 1.0 server may refuse a selection of
/// it, so a `FeatureOfInterest` of a version 1.0 server is expanded without a selection.
fn feature_of_interest_expansion(version: StaVersion) -> String {
    match version {
        StaVersion::V1_0 => FEATURE_OF_INTEREST_MEMBER.to_owned(),
        StaVersion::V1_1 => format!(
            "{FEATURE_OF_INTEREST_MEMBER}($select={ID_SELECTION},{},{ENCODING_TYPE_MEMBER},{FEATURE_MEMBER})",
            CARRIED_FEATURE_OF_INTEREST_MEMBERS.join(",")
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        read_window::ReadWindow,
        request::observations_request::observations_request,
        service_root::ServiceRoot,
        settings::{features_of_interest::FeaturesOfInterest, page_size::PageSize},
        stream_id::StreamId,
    };
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use url::Url;

    fn root() -> ServiceRoot {
        ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap()
    }

    fn window(from: &str, to: &str) -> ReadWindow {
        let instant = |text: &str| -> DateTime<Utc> { DateTime::parse_from_rfc3339(text).unwrap().to_utc() };
        ReadWindow::new(instant(from), instant(to)).unwrap()
    }

    #[test]
    fn a_bounded_observation_request_is_built_exactly() {
        let request = observations_request(
            &root(),
            &StreamId::Datastream(EntityId::Integer(56776)),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::try_from(500).unwrap(),
            FeaturesOfInterest::Omitted,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams(56776)/Observations?$filter=phenomenonTime%20ge%202026-11-11T10:00:00Z%20and%20phenomenonTime%20lt%202026-11-12T10:00:00Z&$orderby=phenomenonTime%20asc,id%20asc&$select=id,phenomenonTime,result,resultTime,validTime,resultQuality,parameters&$top=500"
        );
    }

    #[test]
    fn requested_features_of_interest_of_a_version_1_1_server_are_expanded_with_a_selection_of_their_own() {
        let request = observations_request(
            &root(),
            &StreamId::Datastream(EntityId::Integer(56776)),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
            FeaturesOfInterest::Requested,
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams(56776)/Observations?$filter=phenomenonTime%20ge%202026-11-11T10:00:00Z%20and%20phenomenonTime%20lt%202026-11-12T10:00:00Z&$orderby=phenomenonTime%20asc,id%20asc&$expand=FeatureOfInterest($select=id,name,description,properties,encodingType,feature)&$select=id,phenomenonTime,result,resultTime,validTime,resultQuality,parameters&$top=1000"
        );
    }

    #[test]
    fn requested_features_of_interest_of_a_version_1_0_server_are_expanded_without_a_selection() {
        let request = observations_request(
            &ServiceRoot::try_from(Url::parse("https://api-samenmeten.rivm.nl/v1.0").unwrap()).unwrap(),
            &StreamId::Datastream(EntityId::Integer(56776)),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
            FeaturesOfInterest::Requested,
        );

        assert_eq!(
            request.as_str(),
            "https://api-samenmeten.rivm.nl/v1.0/Datastreams(56776)/Observations?$filter=phenomenonTime%20ge%202026-11-11T10:00:00Z%20and%20phenomenonTime%20lt%202026-11-12T10:00:00Z&$orderby=phenomenonTime%20asc,id%20asc&$expand=FeatureOfInterest&$select=id,phenomenonTime,result,resultTime,validTime,resultQuality,parameters&$top=1000"
        );
    }

    #[test]
    fn the_selection_names_every_property_the_request_orders_by() {
        for features in [FeaturesOfInterest::Omitted, FeaturesOfInterest::Requested] {
            let request = observations_request(
                &root(),
                &StreamId::Datastream(EntityId::Integer(56776)),
                window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
                PageSize::default(),
                features,
            );
            let option = |name: &str| -> String {
                request
                    .query_pairs()
                    .find_map(|(key, value)| (key == name).then(|| value.into_owned()))
                    .unwrap()
            };

            let selection = option("$select");
            let selected: Vec<&str> = selection.split(',').collect();

            for ordered in option("$orderby").split(',') {
                let property = ordered.trim_end_matches(" asc");
                assert!(selected.contains(&property), "{property} of {request}");
            }
        }
    }

    #[test]
    fn a_fractional_bound_keeps_its_fraction() {
        let request = observations_request(
            &root(),
            &StreamId::Datastream(EntityId::Integer(1)),
            window("2026-11-11T10:00:00.250Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
            FeaturesOfInterest::Omitted,
        );

        assert!(
            request.as_str().contains("$filter=phenomenonTime%20ge%202026-11-11T10:00:00.250Z%20and"),
            "{request}"
        );
    }

    #[test]
    fn a_text_id_is_addressed_as_a_string_literal() {
        let request = observations_request(
            &root(),
            &StreamId::Datastream(EntityId::Text("o'neil".into())),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
            FeaturesOfInterest::Omitted,
        );

        assert_eq!(request.path(), "/v1.1/Datastreams('o''neil')/Observations");
    }

    #[test]
    fn the_observations_of_a_multi_datastream_are_requested_from_it() {
        for (features, expand) in [
            (FeaturesOfInterest::Omitted, ""),
            (
                FeaturesOfInterest::Requested,
                "$expand=FeatureOfInterest($select=id,name,description,properties,encodingType,feature)&",
            ),
        ] {
            let request = observations_request(
                &root(),
                &StreamId::MultiDatastream(EntityId::Integer(4)),
                window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
                PageSize::default(),
                features,
            );

            assert_eq!(
                request.as_str(),
                format!(
                    "https://h.example/v1.1/MultiDatastreams(4)/Observations?$filter=phenomenonTime%20ge%202026-11-11T10:00:00Z%20and%20phenomenonTime%20lt%202026-11-12T10:00:00Z&$orderby=phenomenonTime%20asc,id%20asc&{expand}$select=id,phenomenonTime,result,resultTime,validTime,resultQuality,parameters&$top=1000"
                )
            );
        }
    }
}

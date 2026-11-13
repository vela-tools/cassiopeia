use crate::{
    entity_id::EntityId,
    instant_text::instant_text,
    read_window::ReadWindow,
    request::{entity_key::entity_key, query_value::query_value, resource_request::resource_request},
    service_root::ServiceRoot,
    settings::page_size::PageSize,
};
use url::Url;

/// The order Observations are paged in: by `phenomenonTime`, then by id, so Observations of one
/// time keep one order across pages (OGC 18-088 section 9.3.3.1, Req 25).
const BY_TIME_THEN_ID: &str = "phenomenonTime%20asc,id%20asc";

/// The request for the Observations of the data stream `datastream` whose `phenomenonTime` falls in
/// `window`, oldest first, `page_size` a page.
///
/// The bounds are written as RFC 3339 instants in UTC with a `Z`, with a fraction only when they have
/// one. Query parameters are written in the order `$filter`, `$orderby`, `$top`. OGC 18-088 section
/// 9.3.3.5 does not define how a server compares a `phenomenonTime` interval with an instant.
#[must_use]
pub fn observations_request(root: &ServiceRoot, datastream: &EntityId, window: ReadWindow, page_size: PageSize) -> Url {
    let filter = format!(
        "phenomenonTime ge {} and phenomenonTime lt {}",
        instant_text(window.from()),
        instant_text(window.to())
    );
    resource_request(
        root,
        &format!("Datastreams{}/Observations", entity_key(datastream)),
        &format!("$filter={}&$orderby={BY_TIME_THEN_ID}&$top={page_size}", query_value(&filter)),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        read_window::ReadWindow,
        request::observations_request::observations_request,
        service_root::ServiceRoot,
        settings::page_size::PageSize,
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
            &EntityId::Integer(56776),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::try_from(500).unwrap(),
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Datastreams(56776)/Observations?$filter=phenomenonTime%20ge%202026-11-11T10:00:00Z%20and%20phenomenonTime%20lt%202026-11-12T10:00:00Z&$orderby=phenomenonTime%20asc,id%20asc&$top=500"
        );
    }

    #[test]
    fn a_fractional_bound_keeps_its_fraction() {
        let request = observations_request(
            &root(),
            &EntityId::Integer(1),
            window("2026-11-11T10:00:00.250Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
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
            &EntityId::Text("o'neil".into()),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
        );

        assert_eq!(request.path(), "/v1.1/Datastreams('o''neil')/Observations");
    }
}

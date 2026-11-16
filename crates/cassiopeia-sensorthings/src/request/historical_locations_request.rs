use crate::{
    entity_id::EntityId,
    instant_text::instant_text,
    read_window::ReadWindow,
    request::{entity_key::entity_key, query_value::query_value, resource_request::resource_request},
    service_root::ServiceRoot,
    settings::page_size::PageSize,
};
use url::Url;

/// The order `HistoricalLocations` are paged in: by `time`, then by id, so `HistoricalLocations` of one
/// time keep one order across pages (OGC 18-088 section 9.3.3.1, Req 25).
const BY_TIME_THEN_ID: &str = "time%20asc,id%20asc";

/// The request for the `HistoricalLocations` of the Thing `thing` whose `time` falls in `window`,
/// oldest first, `page_size` a page, each with its Locations: `Things({id})/HistoricalLocations`
/// (OGC 18-088 section 8.2.3).
///
/// The Thing is addressed as a data stream is, and the bounds are written as an observations
/// request writes them. Query parameters are written in the order `$filter`, `$expand`, `$orderby`,
/// `$top`.
#[must_use]
pub fn historical_locations_request(root: &ServiceRoot, thing: &EntityId, window: ReadWindow, page_size: PageSize) -> Url {
    let filter = format!("time ge {} and time lt {}", instant_text(window.from()), instant_text(window.to()));
    resource_request(
        root,
        &format!("Things{}/HistoricalLocations", entity_key(thing)),
        &format!("$filter={}&$expand=Locations&$orderby={BY_TIME_THEN_ID}&$top={page_size}", query_value(&filter)),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        read_window::ReadWindow,
        request::historical_locations_request::historical_locations_request,
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
    fn the_historical_locations_of_an_integer_thing_id_are_requested_exactly() {
        let request = historical_locations_request(
            &root(),
            &EntityId::Integer(12512),
            window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z"),
            PageSize::default(),
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Things(12512)/HistoricalLocations?$filter=time%20ge%202026-11-11T10:00:00Z%20and%20time%20lt%202026-11-12T10:00:00Z&$expand=Locations&$orderby=time%20asc,id%20asc&$top=1000"
        );
    }

    #[test]
    fn a_text_thing_id_is_addressed_as_a_string_literal() {
        let request = historical_locations_request(
            &root(),
            &EntityId::Text("bus-12".into()),
            window("2026-11-12T08:30:00.250Z", "2026-11-12T10:00:00Z"),
            PageSize::try_from(2).unwrap(),
        );

        assert_eq!(
            request.as_str(),
            "https://h.example/v1.1/Things('bus-12')/HistoricalLocations?$filter=time%20ge%202026-11-12T08:30:00.250Z%20and%20time%20lt%202026-11-12T10:00:00Z&$expand=Locations&$orderby=time%20asc,id%20asc&$top=2"
        );
    }

    #[test]
    fn a_range_read_and_an_incremental_read_request_each_thing_by_its_bounds() {
        let range = window("2026-11-11T10:00:00Z", "2026-11-12T10:00:00Z");
        let incremental = window("2026-11-12T07:34:00Z", "2026-11-12T10:00:00Z");
        let requested = |thing: EntityId, window: ReadWindow| historical_locations_request(&root(), &thing, window, PageSize::default()).to_string();

        assert_eq!(
            [
                requested(EntityId::Integer(12512), range),
                requested(EntityId::Text("bus-12".into()), range),
                requested(EntityId::Integer(12512), incremental),
                requested(EntityId::Text("bus-12".into()), incremental),
            ],
            [
                "https://h.example/v1.1/Things(12512)/HistoricalLocations?$filter=time%20ge%202026-11-11T10:00:00Z%20and%20time%20lt%202026-11-12T10:00:00Z&$expand=Locations&$orderby=time%20asc,id%20asc&$top=1000",
                "https://h.example/v1.1/Things('bus-12')/HistoricalLocations?$filter=time%20ge%202026-11-11T10:00:00Z%20and%20time%20lt%202026-11-12T10:00:00Z&$expand=Locations&$orderby=time%20asc,id%20asc&$top=1000",
                "https://h.example/v1.1/Things(12512)/HistoricalLocations?$filter=time%20ge%202026-11-12T07:34:00Z%20and%20time%20lt%202026-11-12T10:00:00Z&$expand=Locations&$orderby=time%20asc,id%20asc&$top=1000",
                "https://h.example/v1.1/Things('bus-12')/HistoricalLocations?$filter=time%20ge%202026-11-12T07:34:00Z%20and%20time%20lt%202026-11-12T10:00:00Z&$expand=Locations&$orderby=time%20asc,id%20asc&$top=1000",
            ]
        );
    }
}

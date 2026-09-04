use cassiopeia_common::{attribute_overwrite::AttributeOverwrite, broker_operation::BrokerOperation, upsert_mode::UpsertMode};

/// The NGSI-LD batch upsert endpoint, taking an array of entities per request (ETSI GS CIM 009
/// v1.9.1, clause 5.6.8).
const UPSERT_PATH: &str = "ngsi-ld/v1/entityOperations/upsert";

/// The NGSI-LD batch create endpoint, taking an array of entities per request (clause 5.6.7).
const CREATE_PATH: &str = "ngsi-ld/v1/entityOperations/create";

/// The NGSI-LD batch update endpoint, taking an array of entities per request (clause 5.6.9).
const UPDATE_PATH: &str = "ngsi-ld/v1/entityOperations/update";

/// The NGSI-LD batch merge endpoint, taking an array of entities per request (clause 5.6.20).
const MERGE_PATH: &str = "ngsi-ld/v1/entityOperations/merge";

/// The NGSI-LD temporal entities endpoint, taking a single temporal entity per request (clause
/// 5.6.11).
const TEMPORAL_PATH: &str = "ngsi-ld/v1/temporal/entities";

/// The `?options=update` query selecting update mode on a batch upsert (clause 5.6.8).
const OPTIONS_UPDATE: &str = "options=update";

/// The `?options=noOverwrite` query preserving existing attributes on a batch update (clause 5.6.9).
const OPTIONS_NO_OVERWRITE: &str = "options=noOverwrite";

/// The wire shape a broker operation's request body takes.
///
/// The shape decides both how entities are serialized and whether AIMD batch sizing applies: only
/// [`Array`](RequestShape::Array) posts many entities at once and grows the batch adaptively;
/// [`PerEntity`](RequestShape::PerEntity) posts exactly one entity per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestShape {
    /// A JSON array of entities per request, sized adaptively by the congestion controller.
    Array,
    /// A single entity object per request, one entity per request.
    PerEntity,
}

/// The endpoint path, optional query, and request-body shape an operation posts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationRouting {
    /// The path joined onto the broker base URL.
    pub path: &'static str,
    /// The query string set on the endpoint, when the operation carries a spec option.
    pub query: Option<&'static str>,
    /// The shape each request body takes.
    pub shape: RequestShape,
}

/// Resolves the endpoint path, query, and request shape for a broker operation.
///
/// Every entity operation is an array POST; only the temporal operation is per-entity. The method is
/// always POST, so it is not part of the routing.
#[must_use]
pub const fn route(operation: BrokerOperation) -> OperationRouting {
    match operation {
        BrokerOperation::Upsert(UpsertMode::Replace) => OperationRouting {
            path: UPSERT_PATH,
            query: None,
            shape: RequestShape::Array,
        },
        BrokerOperation::Upsert(UpsertMode::Update) => OperationRouting {
            path: UPSERT_PATH,
            query: Some(OPTIONS_UPDATE),
            shape: RequestShape::Array,
        },
        BrokerOperation::Create => OperationRouting {
            path: CREATE_PATH,
            query: None,
            shape: RequestShape::Array,
        },
        BrokerOperation::Update(AttributeOverwrite::Overwrite) => OperationRouting {
            path: UPDATE_PATH,
            query: None,
            shape: RequestShape::Array,
        },
        BrokerOperation::Update(AttributeOverwrite::NoOverwrite) => OperationRouting {
            path: UPDATE_PATH,
            query: Some(OPTIONS_NO_OVERWRITE),
            shape: RequestShape::Array,
        },
        BrokerOperation::Merge => OperationRouting {
            path: MERGE_PATH,
            query: None,
            shape: RequestShape::Array,
        },
        BrokerOperation::Temporal => OperationRouting {
            path: TEMPORAL_PATH,
            query: None,
            shape: RequestShape::PerEntity,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::request_shape::{OperationRouting, RequestShape, route};
    use cassiopeia_common::{attribute_overwrite::AttributeOverwrite, broker_operation::BrokerOperation, upsert_mode::UpsertMode};
    use url::Url;

    /// Resolves an operation's routing into the endpoint string a `BrokerWriter` would build.
    fn endpoint_for(routing: OperationRouting) -> String {
        let base = Url::parse("https://b/").unwrap();
        let mut endpoint = base.join(routing.path).unwrap();
        endpoint.set_query(routing.query);
        endpoint.into()
    }

    #[test]
    fn a_replacing_upsert_routes_to_the_upsert_endpoint_without_a_query() {
        let routing = route(BrokerOperation::Upsert(UpsertMode::Replace));
        assert_eq!(routing.path, "ngsi-ld/v1/entityOperations/upsert");
        assert_eq!(routing.query, None);
        assert_eq!(routing.shape, RequestShape::Array);
    }

    #[test]
    fn an_updating_upsert_adds_the_options_update_query() {
        let routing = route(BrokerOperation::Upsert(UpsertMode::Update));
        assert_eq!(routing.path, "ngsi-ld/v1/entityOperations/upsert");
        assert_eq!(routing.query, Some("options=update"));
        assert_eq!(routing.shape, RequestShape::Array);
    }

    #[test]
    fn create_routes_to_the_create_endpoint_as_an_array() {
        let routing = route(BrokerOperation::Create);
        assert_eq!(routing.path, "ngsi-ld/v1/entityOperations/create");
        assert_eq!(routing.query, None);
        assert_eq!(routing.shape, RequestShape::Array);
    }

    #[test]
    fn an_overwriting_update_routes_to_the_update_endpoint_without_a_query() {
        let routing = route(BrokerOperation::Update(AttributeOverwrite::Overwrite));
        assert_eq!(routing.path, "ngsi-ld/v1/entityOperations/update");
        assert_eq!(routing.query, None);
        assert_eq!(routing.shape, RequestShape::Array);
    }

    #[test]
    fn a_non_overwriting_update_adds_the_no_overwrite_query() {
        let routing = route(BrokerOperation::Update(AttributeOverwrite::NoOverwrite));
        assert_eq!(routing.path, "ngsi-ld/v1/entityOperations/update");
        assert_eq!(routing.query, Some("options=noOverwrite"));
        assert_eq!(routing.shape, RequestShape::Array);
    }

    #[test]
    fn merge_routes_to_the_merge_endpoint_as_an_array() {
        let routing = route(BrokerOperation::Merge);
        assert_eq!(routing.path, "ngsi-ld/v1/entityOperations/merge");
        assert_eq!(routing.query, None);
        assert_eq!(routing.shape, RequestShape::Array);
    }

    #[test]
    fn temporal_routes_to_the_temporal_endpoint_per_entity() {
        let routing = route(BrokerOperation::Temporal);
        assert_eq!(routing.path, "ngsi-ld/v1/temporal/entities");
        assert_eq!(routing.query, None);
        assert_eq!(routing.shape, RequestShape::PerEntity);
    }

    #[test]
    fn an_updating_upsert_bakes_its_query_into_the_resolved_endpoint() {
        let routing = route(BrokerOperation::Upsert(UpsertMode::Update));
        assert_eq!(endpoint_for(routing), "https://b/ngsi-ld/v1/entityOperations/upsert?options=update");
    }
}

use cassiopeia_common::{broker_header::BrokerHeaders, tenant::Tenant};

/// How a broker writer identifies itself and its tenant on every request.
#[derive(Debug, Clone, Default)]
pub struct BrokerIdentity {
    /// The `NGSILD-Tenant` header value, if any.
    pub tenant: Option<Tenant>,
    /// Extra HTTP headers attached to every request, typically carrying broker credentials.
    pub headers: BrokerHeaders,
}

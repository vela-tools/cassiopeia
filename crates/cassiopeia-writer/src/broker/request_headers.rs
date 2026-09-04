use cassiopeia_common::{broker_header::BrokerHeaders, tenant::Tenant};
use cassiopeia_diagnostic::{
    code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use reqwest::header::{HeaderName, HeaderValue};
use std::sync::Arc;

/// Resolves the tenant into an `NGSILD-Tenant` header, reporting and omitting it when the value is
/// not a legal header value.
///
/// Omitting is the right degradation: the run still delivers, to the broker's default tenant, and the
/// diagnostic says exactly why the tenant was ignored rather than leaving the entities to appear in
/// an unexpected place unexplained.
#[must_use]
pub fn tenant_header(tenant: Option<&Tenant>, sink: &dyn DiagnosticSink) -> Option<HeaderValue> {
    let tenant = tenant?;
    if let Ok(header) = HeaderValue::from_str(tenant.as_str()) {
        return Some(header);
    }

    sink.report(
        &DiagnosticBuilder::new(
            Severity::Warning,
            DiagnosticCode::Broker(BrokerCode::TenantHeaderInvalid),
            "The configured tenant is not a legal HTTP header value; the NGSILD-Tenant header was omitted",
        )
        .build(),
    );
    None
}

/// Marks every credential header sensitive so hyper keeps it out of connection logs.
#[must_use]
pub fn sensitive_auth_headers(headers: &BrokerHeaders) -> Arc<[(HeaderName, HeaderValue)]> {
    headers
        .iter()
        .map(|header| {
            let mut value = header.value().clone();
            value.set_sensitive(true);
            (header.name().clone(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::broker::{request_headers::tenant_header, test_reporter::TestReporter};
    use cassiopeia_common::tenant::Tenant;
    use std::str::FromStr;

    #[test]
    fn a_usable_tenant_becomes_a_header() {
        let reporter = TestReporter::new();

        let header = tenant_header(Some(&Tenant::from_str("acme").unwrap()), &reporter);

        assert_eq!(header.unwrap().to_str().unwrap(), "acme");
        assert!(reporter.diagnostics().is_empty());
    }

    #[test]
    fn no_tenant_produces_no_header_and_no_diagnostic() {
        let reporter = TestReporter::new();

        assert!(tenant_header(None, &reporter).is_none());
        assert!(reporter.diagnostics().is_empty());
    }
}

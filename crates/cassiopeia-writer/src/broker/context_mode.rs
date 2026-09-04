use crate::context_delivery::ContextDelivery;
use cassiopeia_diagnostic::{
    code::{context_code::ContextCode, diagnostic_code::DiagnosticCode},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::context::ContextSource;
use cassiopeia_reporter::reporter::DiagnosticSink;
use reqwest::header::HeaderValue;
use std::sync::Arc;

/// The resolved `@context` handling for a broker target, computed once in `BrokerWriter::new`.
///
/// Doing this once keeps the worker loop and the flush path from repeating the delivery/context
/// combinatorics on every batch. The `BodyInjected` arm shares its `ContextSource` through an `Arc`
/// so every worker thread reads the same backing map rather than cloning it per job.
#[derive(Clone)]
pub enum ContextMode {
    /// No `@context`: `application/json` with no `Link` header.
    None,
    /// `application/json` plus a `Link` header pointing at the context URL.
    LinkHeader(HeaderValue),
    /// `application/ld+json`: the context is injected into each entity body before serialization.
    BodyInjected(Arc<ContextSource>),
}

impl ContextMode {
    /// The `Content-Type` header this mode sends.
    pub const fn content_type(&self) -> HeaderValue {
        match self {
            ContextMode::BodyInjected(_) => HeaderValue::from_static("application/ld+json"),
            ContextMode::None | ContextMode::LinkHeader(_) => HeaderValue::from_static("application/json"),
        }
    }

    /// The `Link` header this mode sends, if any.
    pub fn link_header(&self) -> Option<HeaderValue> {
        match self {
            ContextMode::LinkHeader(value) => Some(value.clone()),
            ContextMode::None | ContextMode::BodyInjected(_) => None,
        }
    }

    /// The shared context source workers inject into entity bodies, present only in body mode.
    pub fn body_context(&self) -> Option<Arc<ContextSource>> {
        match self {
            ContextMode::BodyInjected(source) => Some(Arc::clone(source)),
            ContextMode::None | ContextMode::LinkHeader(_) => None,
        }
    }
}

/// Resolves the effective context mode from the requested delivery and the context source.
///
/// Link-header delivery only works when the context is a single URL string; when it is requested but
/// the context is not such a URL, the resolver reports the degradation and falls back to body-context
/// mode so a misconfiguration degrades rather than fails.
pub fn resolve_context_mode(delivery: ContextDelivery, context: ContextSource, sink: &dyn DiagnosticSink) -> ContextMode {
    if delivery == ContextDelivery::LinkHeader {
        if let ContextSource::Static(context) = &context
            && let Some(url) = context.as_url()
        {
            let value = format!("<{url}>; rel=\"http://www.w3.org/ns/json-ld#context\"");
            match HeaderValue::from_str(&value) {
                Ok(header) => return ContextMode::LinkHeader(header),
                Err(_) => report(
                    sink,
                    ContextCode::LinkHeaderInvalid,
                    "The @context URL is not a legal HTTP header value; delivering the context in each entity body instead",
                ),
            }
        } else {
            report(
                sink,
                ContextCode::LinkDeliveryUnsupported,
                "Link-header @context delivery was requested but the context is not a single URL; delivering it in each entity body instead",
            );
        }
    }

    if matches!(context, ContextSource::None) {
        ContextMode::None
    } else {
        ContextMode::BodyInjected(Arc::new(context))
    }
}

/// Reports one degradation of the requested `@context` delivery.
fn report(sink: &dyn DiagnosticSink, code: ContextCode, headline: &str) {
    sink.report(&DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Context(code), headline).build());
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::{
            context_mode::{ContextMode, resolve_context_mode},
            test_reporter::TestReporter,
        },
        context_delivery::ContextDelivery,
    };
    use cassiopeia_diagnostic::code::{context_code::ContextCode, diagnostic_code::DiagnosticCode};
    use cassiopeia_ngsi_ld::entity::context::{ContextSource, NgsiLdContext};
    use url::Url;

    #[test]
    fn link_delivery_with_an_unusable_context_warns_and_falls_back() {
        let reporter = TestReporter::new();

        let mode = resolve_context_mode(ContextDelivery::LinkHeader, ContextSource::None, &reporter);

        assert!(matches!(mode, ContextMode::None));
        assert_eq!(reporter.diagnostics().len(), 1);
        assert_eq!(reporter.diagnostics()[0].code, DiagnosticCode::Context(ContextCode::LinkDeliveryUnsupported));
    }

    #[test]
    fn link_delivery_with_a_single_url_builds_the_link_header() {
        let reporter = TestReporter::new();

        let mode = resolve_context_mode(
            ContextDelivery::LinkHeader,
            ContextSource::Static(NgsiLdContext::remote(Url::parse("https://example.com/ctx.jsonld").unwrap())),
            &reporter,
        );

        match mode {
            ContextMode::LinkHeader(value) => {
                let rendered = value.to_str().unwrap();
                assert!(rendered.contains("https://example.com/ctx.jsonld"));
                assert!(rendered.contains("rel=\"http://www.w3.org/ns/json-ld#context\""));
            }
            ContextMode::None | ContextMode::BodyInjected(_) => panic!("expected a link header"),
        }
    }
}

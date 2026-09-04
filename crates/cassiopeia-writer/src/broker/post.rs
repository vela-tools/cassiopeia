use crate::broker::{
    batch_settlement::{RetryStep, on_retryable, on_transport_error, settle_opaque, settle_partial, settle_rejected, settle_too_large, settle_unreadable},
    delivery_report::DeliveryContext,
    response_verdict::{ResponseVerdict, interpret_response},
    sender::SenderContext,
};
use bytes::Bytes;
use cassiopeia_common::captured_body::CapturedBody;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use http::StatusCode;
use reqwest::{
    blocking::{RequestBuilder, Response},
    header,
};
use std::{sync::atomic::Ordering, time::Instant};

/// The cap on captured response-body bytes. It is generous because a conformant NGSI-LD `detail`
/// (ETSI GS CIM 009 v1.9.1 clause 5.5.3) can list every offending attribute of a large batch, and a
/// 207 result body names every entity of it; a capture that cut those short would make the body
/// unparseable and lose the diagnostics entirely.
const RESPONSE_CAPTURE_CAP: usize = 64 * 1024;

/// The cap on the body echoed into a diagnostic when it did not parse. Far smaller than the capture
/// cap: an echo is read by a person, and a screenful is all one can use.
pub(crate) const BODY_ECHO_CAP: usize = 2 * 1024;

/// What a POST attempt loop decided about a batch.
pub enum SendOutcome {
    /// The batch was handled (written, or counted as failed); nothing more to do.
    Done,
    /// The batch must be split and its halves retried.
    Split,
}

/// Runs the POST-with-retry loop for one serialized batch.
///
/// The entities are borrowed rather than consumed: the caller owns the `Vec` across the call because
/// it may have to split it, and a pointer plus a length is what lets a diagnostic name the entities a
/// failed request carried.
pub fn post_batch(ctx: &SenderContext, payload: &Bytes, entities: &[NgsiLdEntity]) -> SendOutcome {
    let entity_count = entities.len();
    let payload_len = payload.len();
    let delivery = DeliveryContext {
        endpoint: &ctx.request.endpoint,
        entities,
    };
    let mut attempt: u32 = 0;

    loop {
        if ctx.runtime.shutdown.load(Ordering::Acquire) {
            ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
            return SendOutcome::Done;
        }

        let started = Instant::now();
        let response = match build_request(ctx, payload).send() {
            Ok(response) => response,
            Err(source) => match on_transport_error(ctx, &delivery, source, &mut attempt) {
                RetryStep::Again => continue,
                RetryStep::Settled(outcome) => return outcome,
            },
        };

        let status = response.status();
        // A full-success response carries no body; every other status is read so the broker's own
        // problem details (clause 6.3.3 requires them) reach the report.
        let body = if status.is_success() && status != StatusCode::MULTI_STATUS {
            CapturedBody::capped(Vec::new(), RESPONSE_CAPTURE_CAP)
        } else {
            read_body(response, RESPONSE_CAPTURE_CAP)
        };

        match interpret_response(status, &body, entity_count) {
            ResponseVerdict::Written(count) => {
                record_healthy(ctx, started, payload_len);
                ctx.counters.written.fetch_add(count, Ordering::Relaxed);
                return SendOutcome::Done;
            }
            ResponseVerdict::Partial(result) => {
                record_healthy(ctx, started, payload_len);
                return settle_partial(ctx, &delivery, &result, status);
            }
            ResponseVerdict::UnreadableMultiStatus { source } => {
                record_healthy(ctx, started, payload_len);
                return settle_unreadable(ctx, &delivery, source, &body, status);
            }
            ResponseVerdict::TooLarge { problem } => return settle_too_large(ctx, &delivery, problem.as_ref(), status, payload_len),
            ResponseVerdict::Retryable { status, problem } => match on_retryable(ctx, &delivery, status, problem.as_ref(), payload_len, &mut attempt) {
                RetryStep::Again => {}
                RetryStep::Settled(outcome) => return outcome,
            },
            ResponseVerdict::Rejected { status, problem } => return settle_rejected(ctx, &delivery, status, &problem),
            ResponseVerdict::RejectedOpaque { status } => return settle_opaque(ctx, &delivery, status, &body),
        }
    }
}

/// Builds a POST request with the content-type, link, tenant, and auth headers, and the payload body.
fn build_request(ctx: &SenderContext, payload: &Bytes) -> RequestBuilder {
    let mut request = ctx
        .request
        .client
        .post(ctx.request.endpoint.as_str())
        .header(header::CONTENT_TYPE, ctx.request.content_type.clone());
    if let Some(link) = &ctx.request.link_header {
        request = request.header(header::LINK, link.clone());
    }
    if let Some(tenant) = &ctx.request.tenant_header {
        request = request.header("NGSILD-Tenant", tenant.clone());
    }
    for (name, value) in ctx.request.auth_headers.iter() {
        request = request.header(name.clone(), value.clone());
    }
    request.body(payload.clone())
}

/// Records the congestion signals of a healthy response: its latency and the bytes it accepted.
fn record_healthy(ctx: &SenderContext, started: Instant, payload_len: usize) {
    let elapsed = started.elapsed();
    ctx.counters
        .metrics
        .request_time_ns
        .fetch_add(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
    ctx.counters.metrics.record_success(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
    ctx.counters
        .metrics
        .bytes_delivered
        .fetch_add(u64::try_from(payload_len).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// Reads a response body into a capped capture, recording a read failure as such rather than as an
/// empty body: an empty body and one that could not be read say very different things.
fn read_body(response: Response, cap: usize) -> CapturedBody {
    match response.bytes() {
        Ok(bytes) => CapturedBody::capped(bytes.to_vec(), cap),
        Err(source) => CapturedBody::unreadable(&source),
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::{
        batch_settlement::report_entity_dropped,
        post::{SendOutcome, post_batch},
        request_shape::RequestShape,
        sender::test_support::{context, thing},
        test_reporter::TestReporter,
    };
    use bytes::Bytes;
    use cassiopeia_diagnostic::code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode};
    use std::{net::TcpListener, sync::Arc};
    use url::Url;

    /// A URL nothing is listening on, so every request fails at connect without a server to run.
    fn closed_endpoint() -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a bindable port");
        let port = listener.local_addr().expect("a local address").port();
        drop(listener);
        Url::parse(&format!("http://127.0.0.1:{port}/ngsi-ld/v1/entityOperations/upsert")).expect("a valid URL")
    }

    #[test]
    fn a_retry_storm_reports_once_on_exhaustion_rather_than_once_per_attempt() {
        static REPORTER: TestReporter = TestReporter::new();
        let mut ctx = context(RequestShape::Array);
        ctx.request.endpoint = Arc::new(closed_endpoint());
        ctx.runtime.max_retries = 2;
        ctx.runtime.reporter = &REPORTER;

        let entities = vec![thing("urn:ngsi-ld:Thing:1"), thing("urn:ngsi-ld:Thing:2")];
        let outcome = post_batch(&ctx, &Bytes::from_static(b"[]"), &entities);

        assert!(matches!(outcome, SendOutcome::Split));
        let reported = REPORTER.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Broker(BrokerCode::TransportFailed));
        assert!(reported[0].headline.contains("2 attempts"));
        assert!(!ctx.counters.failures.is_empty());
    }

    #[test]
    fn a_single_entity_dropped_after_retries_names_the_entity() {
        static REPORTER: TestReporter = TestReporter::new();
        let mut ctx = context(RequestShape::Array);
        ctx.runtime.reporter = &REPORTER;

        report_entity_dropped(&ctx, &thing("urn:ngsi-ld:AirQualityObserved:LJ-001"));

        let reported = REPORTER.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Broker(BrokerCode::EntityDropped));
        assert!(reported[0].headline.contains("urn:ngsi-ld:AirQualityObserved:LJ-001"));
    }
}

use crate::broker::{batch_result::BatchOperationResult, broker_rejection::BrokerRejection, problem_details::ProblemDetails};
use cassiopeia_common::captured_body::CapturedBody;
use http::StatusCode;

/// How a broker response maps onto the writer's accounting, derived purely from the status and body.
#[derive(Debug)]
pub enum ResponseVerdict {
    /// A full-success response (201/204): the whole batch was written.
    Written(usize),
    /// A 207 Multi-Status naming which entities were accepted and which were not.
    Partial(BatchOperationResult),
    /// A 207 Multi-Status whose body could not be read, so which entities were written is unknown.
    UnreadableMultiStatus {
        /// The parse failure, kept so the report can name what the body looked like.
        source: serde_json::Error,
    },
    /// A 5xx or 429: retry with backoff, then split on exhaustion.
    Retryable {
        /// The status the broker answered with.
        status: StatusCode,
        /// The broker's explanation, when it sent a problem body.
        problem: Option<BrokerRejection>,
    },
    /// A 413 Payload Too Large: split and retry the smaller halves.
    TooLarge {
        /// The broker's explanation, when it sent a problem body.
        problem: Option<BrokerRejection>,
    },
    /// Any other 4xx with a problem body: a schema, auth, or tenant bug the broker explained.
    Rejected {
        /// The status the broker answered with.
        status: StatusCode,
        /// The broker's explanation.
        problem: BrokerRejection,
    },
    /// Any other 4xx whose body is not problem details, typically an error page from a proxy in
    /// front of the broker rather than the broker's own refusal.
    RejectedOpaque {
        /// The status the response carried.
        status: StatusCode,
    },
}

/// Interprets a broker response into a verdict, parsing the body for every non-success status.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.3.3 requires an RFC 7807 body on every error response and clause
/// 5.5.3 requires its `detail` to convey enough information to act on, so the body is read for a 4xx
/// and a 5xx alike and not only for a 207.
///
/// A 207 whose body cannot be read is *not* counted as written. The specification guarantees partial
/// failure on that status, so trusting an unreadable body would silently record rejected entities as
/// delivered: the one outcome a data producer must never report.
#[must_use]
pub fn interpret_response(status: StatusCode, body: &CapturedBody, entity_count: usize) -> ResponseVerdict {
    if status == StatusCode::MULTI_STATUS {
        return match serde_json::from_slice::<BatchOperationResult>(body.bytes()) {
            Ok(result) => ResponseVerdict::Partial(result),
            Err(source) => ResponseVerdict::UnreadableMultiStatus { source },
        };
    }
    if status.is_success() {
        return ResponseVerdict::Written(entity_count);
    }

    let problem = serde_json::from_slice::<ProblemDetails>(body.bytes()).ok().map(BrokerRejection::new);
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return ResponseVerdict::TooLarge { problem };
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return ResponseVerdict::Retryable { status, problem };
    }
    match problem {
        Some(problem) => ResponseVerdict::Rejected { status, problem },
        None => ResponseVerdict::RejectedOpaque { status },
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::response_verdict::{ResponseVerdict, interpret_response};
    use cassiopeia_common::captured_body::CapturedBody;
    use http::StatusCode;

    fn body(bytes: &[u8]) -> CapturedBody {
        CapturedBody::capped(bytes.to_vec(), 64 * 1024)
    }

    fn status(code: u16) -> StatusCode {
        StatusCode::from_u16(code).expect("a valid status")
    }

    #[test]
    fn a_full_success_status_writes_the_whole_batch() {
        assert!(matches!(interpret_response(status(201), &body(b""), 5), ResponseVerdict::Written(5)));
        assert!(matches!(interpret_response(status(204), &body(b""), 5), ResponseVerdict::Written(5)));
    }

    #[test]
    fn a_207_splits_the_written_from_the_failed() {
        let payload = br#"{"success":["urn:ngsi-ld:A","urn:ngsi-ld:B"],"errors":[{"entityId":"urn:ngsi-ld:C","error":{"type":"urn:ex","status":409}}]}"#;

        let ResponseVerdict::Partial(result) = interpret_response(status(207), &body(payload), 3) else {
            panic!("expected a partial verdict");
        };
        assert_eq!(result.success.len(), 2);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn a_207_with_an_unparseable_body_is_never_counted_as_written() {
        let verdict = interpret_response(status(207), &body(b"not json"), 4);

        assert!(!matches!(verdict, ResponseVerdict::Written(_)));
        let ResponseVerdict::UnreadableMultiStatus { source } = verdict else {
            panic!("expected an unreadable multi-status verdict");
        };
        assert!(!source.to_string().is_empty());
    }

    #[test]
    fn a_truncated_207_is_unreadable_rather_than_written() {
        let payload = br#"{"success":["urn:ngsi-ld:A","urn:ngsi-ld:B"],"errors":[]}"#;
        let truncated = CapturedBody::capped(payload.to_vec(), 20);

        assert!(matches!(
            interpret_response(status(207), &truncated, 2),
            ResponseVerdict::UnreadableMultiStatus { .. }
        ));
    }

    #[test]
    fn a_413_is_too_large_and_carries_the_brokers_explanation() {
        let ResponseVerdict::TooLarge { problem } = interpret_response(status(413), &body(br#"{"detail":"body over 1MiB"}"#), 8) else {
            panic!("expected a too-large verdict");
        };
        assert_eq!(problem.expect("a problem body").text(), "body over 1MiB");
    }

    #[test]
    fn congestion_and_server_failures_are_retryable() {
        assert!(matches!(interpret_response(status(429), &body(b""), 8), ResponseVerdict::Retryable { .. }));
        assert!(matches!(interpret_response(status(503), &body(b""), 8), ResponseVerdict::Retryable { .. }));
    }

    #[test]
    fn a_422_with_a_detail_only_body_is_a_typed_rejection() {
        let payload = br#"{"detail":"attribute 'dateObserved' is not a valid DateTime"}"#;

        let ResponseVerdict::Rejected { status: code, problem } = interpret_response(status(422), &body(payload), 100) else {
            panic!("expected a typed rejection");
        };
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(problem.text(), "attribute 'dateObserved' is not a valid DateTime");
    }

    #[test]
    fn a_400_with_an_html_body_is_opaque() {
        assert!(matches!(
            interpret_response(status(400), &body(b"<html><body>Bad Request</body></html>"), 8),
            ResponseVerdict::RejectedOpaque { .. }
        ));
    }

    #[test]
    fn an_authentication_failure_is_a_rejection_rather_than_a_retry() {
        assert!(matches!(
            interpret_response(status(401), &body(br#"{"title":"Unauthorized"}"#), 8),
            ResponseVerdict::Rejected { .. }
        ));
    }
}

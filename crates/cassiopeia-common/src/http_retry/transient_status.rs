use http::StatusCode;

/// Returns whether `status` says the same request may succeed when sent again later.
///
/// That is `429 Too Many Requests` (RFC 6585 section 4), a server limiting how often it is asked,
/// and every 5xx (RFC 9110 section 15.6), a server failing to answer a request it may well serve on
/// another try. Every other 4xx points at the request itself (a wrong URL, a missing key, a refused
/// body), so sending it again cannot help.
#[must_use]
pub fn is_transient(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use crate::http_retry::transient_status::is_transient;
    use http::StatusCode;

    #[test]
    fn too_many_requests_is_transient() {
        assert!(is_transient(StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn every_server_error_is_transient() {
        assert!(is_transient(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_transient(StatusCode::BAD_GATEWAY));
        assert!(is_transient(StatusCode::SERVICE_UNAVAILABLE));
        assert!(is_transient(StatusCode::GATEWAY_TIMEOUT));
    }

    #[test]
    fn any_other_client_error_is_final() {
        assert!(!is_transient(StatusCode::BAD_REQUEST));
        assert!(!is_transient(StatusCode::UNAUTHORIZED));
        assert!(!is_transient(StatusCode::NOT_FOUND));
        assert!(!is_transient(StatusCode::PAYLOAD_TOO_LARGE));
    }

    #[test]
    fn a_success_or_redirect_is_not_transient() {
        assert!(!is_transient(StatusCode::OK));
        assert!(!is_transient(StatusCode::FOUND));
    }
}

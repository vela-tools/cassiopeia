use http::{HeaderMap, header::RETRY_AFTER};
use std::time::{Duration, SystemTime};

/// The least time a server asked a client to wait before its next request, read from a
/// `Retry-After` header.
///
/// RFC 9110 section 10.2.3 writes the header as either a whole number of seconds or an HTTP-date. A
/// date is turned into the time left until it, so a date already past asks for no wait at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RetryAfter(Duration);

impl RetryAfter {
    /// Wraps the least time to wait.
    #[must_use]
    pub const fn new(delay: Duration) -> RetryAfter {
        RetryAfter(delay)
    }

    /// Reads the `Retry-After` header of a response, if it carries one that parses. A value that is
    /// neither form names no wait, so it is ignored as if the header were absent.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Option<RetryAfter> {
        let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
        RetryAfter::parse(value, SystemTime::now())
    }

    /// Reads a `Retry-After` value as of `now`.
    fn parse(value: &str, now: SystemTime) -> Option<RetryAfter> {
        let value = value.trim();
        // `delay-seconds = 1*DIGIT`, so a sign or a fraction makes the value no delay at all.
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            return value.parse().ok().map(|seconds| RetryAfter(Duration::from_secs(seconds)));
        }
        let date = httpdate::parse_http_date(value).ok()?;
        Some(RetryAfter(date.duration_since(now).unwrap_or(Duration::ZERO)))
    }

    /// Returns the least time to wait.
    #[must_use]
    pub const fn delay(self) -> Duration {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::http_retry::retry_after::RetryAfter;
    use http::{HeaderMap, HeaderValue, header::RETRY_AFTER};
    use pretty_assertions::assert_eq;
    use std::time::{Duration, SystemTime};

    /// A fixed instant, Sun, 06 Nov 1994 08:49:37 GMT, the example date of RFC 9110 section 5.6.7.
    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_777)
    }

    #[test]
    fn a_number_of_seconds_is_the_wait() {
        assert_eq!(RetryAfter::parse("120", now()), Some(RetryAfter::new(Duration::from_secs(120))));
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(RetryAfter::parse(" 5 ", now()), Some(RetryAfter::new(Duration::from_secs(5))));
    }

    #[test]
    fn an_http_date_waits_until_that_date() {
        assert_eq!(
            RetryAfter::parse("Sun, 06 Nov 1994 08:51:37 GMT", now()),
            Some(RetryAfter::new(Duration::from_secs(120)))
        );
    }

    #[test]
    fn an_http_date_already_past_asks_for_no_wait() {
        assert_eq!(RetryAfter::parse("Sun, 06 Nov 1994 08:00:00 GMT", now()), Some(RetryAfter::new(Duration::ZERO)));
    }

    #[test]
    fn a_signed_or_fractional_number_is_no_wait() {
        assert_eq!(RetryAfter::parse("+5", now()), None);
        assert_eq!(RetryAfter::parse("1.5", now()), None);
        assert_eq!(RetryAfter::parse("-1", now()), None);
    }

    #[test]
    fn a_value_in_neither_form_is_no_wait() {
        assert_eq!(RetryAfter::parse("soon", now()), None);
        assert_eq!(RetryAfter::parse("", now()), None);
    }

    #[test]
    fn a_response_header_is_read() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));

        assert_eq!(RetryAfter::from_headers(&headers), Some(RetryAfter::new(Duration::from_secs(7))));
    }

    #[test]
    fn a_response_without_the_header_names_no_wait() {
        assert_eq!(RetryAfter::from_headers(&HeaderMap::new()), None);
    }
}

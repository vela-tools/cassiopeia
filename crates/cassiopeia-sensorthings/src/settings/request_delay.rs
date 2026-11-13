use humantime_serde::Serde;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// The longest wait a read may keep between two requests.
const MAX_DELAY: Duration = Duration::from_mins(1);

/// A wait between consecutive requests of a read, written the way `humantime` reads it, such as
/// `200ms`, and at most a minute.
///
/// It spares a public server a burst of requests: every request of a cycle but the first waits this
/// long after the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Serde<Duration>")]
pub struct RequestDelay(#[serde(with = "humantime_serde")] Duration);

/// Why a duration is no request delay.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("`delay` is at most 60s, not {}", humantime::format_duration(*.0))]
pub struct RequestDelayError(Duration);

impl RequestDelay {
    /// How long a request waits after the one before it.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl TryFrom<Duration> for RequestDelay {
    type Error = RequestDelayError;

    fn try_from(delay: Duration) -> Result<RequestDelay, RequestDelayError> {
        if delay > MAX_DELAY {
            return Err(RequestDelayError(delay));
        }
        Ok(RequestDelay(delay))
    }
}

impl TryFrom<Serde<Duration>> for RequestDelay {
    type Error = RequestDelayError;

    fn try_from(delay: Serde<Duration>) -> Result<RequestDelay, RequestDelayError> {
        RequestDelay::try_from(delay.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::request_delay::RequestDelay;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn a_delay_is_read_as_humantime_writes_it() {
        let delay: RequestDelay = serde_json::from_str(r#""200ms""#).unwrap();

        assert_eq!(delay.get(), Duration::from_millis(200));
        assert_eq!(serde_json::to_string(&delay).unwrap(), r#""200ms""#);
    }

    #[test]
    fn a_delay_of_a_minute_is_the_longest() {
        assert_eq!(serde_json::from_str::<RequestDelay>(r#""60s""#).unwrap().get(), Duration::from_mins(1));

        let error = serde_json::from_str::<RequestDelay>(r#""61s""#).unwrap_err();
        assert!(error.to_string().contains("`delay` is at most 60s, not 1m 1s"), "{error}");
    }
}

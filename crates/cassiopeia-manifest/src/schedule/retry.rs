use getset::Getters;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use typed_builder::TypedBuilder;

/// How long the scheduler waits before retrying a failed run when the manifest does not say.
const DEFAULT_BACKOFF: Duration = Duration::from_secs(5);

/// How a failed run is retried before the schedule gives up on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    /// How many times a failed run is attempted again. Zero attempts is meaningless, so the count
    /// is non-zero by construction.
    #[getset(get = "pub")]
    max_attempts: NonZeroU32,

    /// How long to wait between attempts, written the way `humantime` reads it, such as `5s`.
    #[serde(with = "humantime_serde", default = "default_backoff")]
    #[builder(default = DEFAULT_BACKOFF)]
    #[getset(get = "pub")]
    backoff: Duration,
}

/// Supplies the default backoff to serde, which needs a function rather than a constant.
const fn default_backoff() -> Duration {
    DEFAULT_BACKOFF
}

impl RetryPolicy {
    /// Builds a retry policy from optional attempt-count and backoff settings.
    ///
    /// Returns no policy when no attempt count is given, since retrying is meaningless without a
    /// bound on attempts; an absent backoff falls back to [`DEFAULT_BACKOFF`].
    #[must_use]
    pub fn new(max_attempts: Option<NonZeroU32>, backoff: Option<Duration>) -> Option<RetryPolicy> {
        let max_attempts = max_attempts?;
        let builder = RetryPolicy::builder().max_attempts(max_attempts);

        Some(match backoff {
            Some(backoff) => builder.backoff(backoff).build(),
            None => builder.build(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::schedule::retry::RetryPolicy;
    use std::{num::NonZeroU32, time::Duration};

    #[test]
    fn reads_a_policy_stating_both_settings() {
        let policy: RetryPolicy = serde_json::from_str(r#"{"maxAttempts": 3, "backoff": "10s"}"#).unwrap();

        assert_eq!(policy.max_attempts(), &NonZeroU32::new(3).unwrap());
        assert_eq!(policy.backoff(), &Duration::from_secs(10));
    }

    #[test]
    fn an_unstated_backoff_falls_back_to_five_seconds() {
        let policy: RetryPolicy = serde_json::from_str(r#"{"maxAttempts": 1}"#).unwrap();

        assert_eq!(policy.backoff(), &Duration::from_secs(5));
    }

    #[test]
    fn a_policy_without_an_attempt_count_is_rejected() {
        assert!(serde_json::from_str::<RetryPolicy>(r#"{"backoff": "10s"}"#).is_err());
    }

    #[test]
    fn a_zero_attempt_count_is_rejected() {
        assert!(serde_json::from_str::<RetryPolicy>(r#"{"maxAttempts": 0}"#).is_err());
    }

    #[test]
    fn the_constructor_needs_an_attempt_count() {
        assert_eq!(RetryPolicy::new(None, Some(Duration::from_secs(10))), None);
    }

    #[test]
    fn an_unstated_backoff_falls_back_to_the_default() {
        let policy = RetryPolicy::new(NonZeroU32::new(3), None).unwrap();

        assert_eq!(policy.max_attempts(), &NonZeroU32::new(3).unwrap());
        assert_eq!(policy.backoff(), &Duration::from_secs(5));
    }

    #[test]
    fn a_stated_backoff_is_carried() {
        let policy = RetryPolicy::new(NonZeroU32::new(2), Some(Duration::from_secs(20))).unwrap();

        assert_eq!(policy.backoff(), &Duration::from_secs(20));
    }
}

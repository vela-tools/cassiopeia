use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::{num::NonZeroUsize, time::Duration};

// Both counts are built up from `NonZeroUsize::MIN` so the compiler proves them non-zero without
// an unwrap at the constant.
/// How many sources are fetched at once when the configuration does not say.
const DEFAULT_MAX_CONCURRENT_DOWNLOADS: NonZeroUsize = NonZeroUsize::MIN.saturating_add(49);
/// How many idle connections the HTTP client keeps when the configuration does not say.
const DEFAULT_CONNECTION_POOL_MAX_IDLE: NonZeroUsize = NonZeroUsize::MIN.saturating_add(5);

/// How the collector fetches remote sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct Download {
    /// How many times a failed request is attempted again.
    #[default = 3]
    pub max_retries: u32,

    /// How long to wait before the first retry; each further retry waits longer.
    #[serde(with = "humantime_serde")]
    #[default(Duration::from_millis(500))]
    pub initial_backoff: Duration,

    /// How many sources are fetched at once.
    #[default(DEFAULT_MAX_CONCURRENT_DOWNLOADS)]
    pub max_concurrent_downloads: NonZeroUsize,

    /// How long a single request may take before it is abandoned.
    #[serde(with = "humantime_serde")]
    #[default(Duration::from_secs(30))]
    pub http_timeout: Duration,

    /// How many idle connections the HTTP client keeps open for reuse.
    #[default(DEFAULT_CONNECTION_POOL_MAX_IDLE)]
    pub connection_pool_max_idle: NonZeroUsize,
}

#[cfg(test)]
mod tests {
    use crate::download::Download;
    use std::time::Duration;

    const EXPLICIT: &str = r#"
        max_retries = 1
        initial_backoff = "250ms"
        max_concurrent_downloads = 4
        http_timeout = "5s"
        connection_pool_max_idle = 2
    "#;

    #[test]
    fn the_defaults_describe_a_patient_but_bounded_client() {
        let download = Download::default();

        assert_eq!(download.max_retries, 3);
        assert_eq!(download.initial_backoff, Duration::from_millis(500));
        assert_eq!(download.http_timeout, Duration::from_secs(30));
        assert_eq!(download.max_concurrent_downloads.get(), 50);
        assert_eq!(download.connection_pool_max_idle.get(), 6);
    }

    #[test]
    fn durations_are_written_the_way_humantime_reads_them() {
        let download: Download = toml::from_str(EXPLICIT).unwrap();

        assert_eq!(download.initial_backoff, Duration::from_millis(250));
        assert_eq!(download.http_timeout, Duration::from_secs(5));
    }

    #[test]
    fn a_zero_concurrency_is_rejected() {
        assert!(toml::from_str::<Download>(&EXPLICIT.replace("max_concurrent_downloads = 4", "max_concurrent_downloads = 0")).is_err());
    }
}

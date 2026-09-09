use crate::broker::{http2_prior_knowledge::Http2PriorKnowledge, tuning::BrokerTuning};
use cassiopeia_common::user_agent::UserAgent;
use std::time::Duration;
use url::Url;

/// The default number of background sender threads, sized so a handful in retry-backoff cannot
/// starve the rest of the pool.
const DEFAULT_SENDER_THREADS: usize = 8;

/// The default HTTP request timeout. 30s covers realistic broker latency while failing a hung
/// request fast enough that retries still help.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The default TCP keepalive interval.
const DEFAULT_TCP_KEEPALIVE: Duration = Duration::from_mins(1);

/// The default reqwest idle-connection pool size per host: one idle connection per worker plus
/// headroom for a worker cycling through a reconnect.
const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 16;

/// The default retry ceiling. Three attempts at the capped backoff (1s + 2s + 4s ≈ 7s) bound the
/// worst-case per-job block; the controller handles sustained distress by shrinking the target.
const DEFAULT_MAX_RETRIES: u32 = 3;

/// The multiplier on `sender_threads` for the default channel capacity, so the main thread does not
/// block on `send()` while workers are mid-flight.
const DEFAULT_CHANNEL_CAPACITY_MULTIPLIER: usize = 4;

/// The HTTP transport settings a broker writer builds its client and worker pool from.
pub struct BrokerTransport {
    /// The broker base URL the operation endpoint is joined onto.
    pub base_url: Url,
    /// The `User-Agent` header sent with every request.
    pub user_agent: UserAgent,
    /// The per-request HTTP timeout.
    pub timeout: Duration,
    /// The reqwest idle-connection pool size per host.
    pub pool_max_idle_per_host: usize,
    /// The TCP keepalive interval.
    pub tcp_keepalive: Duration,
    /// How many background sender threads to run.
    pub sender_threads: usize,
    /// The bounded channel capacity between the main thread and the workers.
    pub channel_capacity: usize,
    /// The retry ceiling before a batch is split or dropped.
    pub max_retries: u32,
    /// Whether to force HTTP/2 without ALPN; required only for plaintext h2c brokers.
    pub http2_prior_knowledge: Http2PriorKnowledge,
    /// The adaptive-controller tuning.
    pub tuning: BrokerTuning,
}

impl BrokerTransport {
    /// Builds the transport settings from the broker base URL and user-agent, filling every other
    /// knob from its default.
    #[must_use]
    pub fn new(base_url: Url, user_agent: UserAgent) -> BrokerTransport {
        let sender_threads = DEFAULT_SENDER_THREADS;
        BrokerTransport {
            base_url,
            user_agent,
            timeout: DEFAULT_TIMEOUT,
            pool_max_idle_per_host: DEFAULT_POOL_MAX_IDLE_PER_HOST,
            tcp_keepalive: DEFAULT_TCP_KEEPALIVE,
            sender_threads,
            channel_capacity: sender_threads * DEFAULT_CHANNEL_CAPACITY_MULTIPLIER,
            max_retries: DEFAULT_MAX_RETRIES,
            http2_prior_knowledge: Http2PriorKnowledge::default(),
            tuning: BrokerTuning::default(),
        }
    }
}

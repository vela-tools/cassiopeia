use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Congestion-control telemetry shared between the worker threads (writers) and the main thread
/// (reader).
///
/// Every field is a best-effort smoothed metric; a brief race under concurrent updates only delays
/// a signal by one observation, which is indistinguishable from EWMA noise, so the updates are plain
/// load/compute/store rather than CAS loops.
#[derive(Debug, Default)]
pub struct BrokerMetrics {
    /// EWMA of successful-request latency in milliseconds. `0` means no sample yet.
    pub avg_latency_ms: AtomicU64,
    /// EWMA of observed per-entity serialized size in bytes, read by the main thread when sizing the
    /// next batch. `0` means no sample yet.
    pub avg_entity_size_bytes: AtomicUsize,
    /// Consecutive successful requests since the last failure.
    pub consecutive_successes: AtomicU32,
    /// Consecutive failed requests (5xx / 429 / timeout / transport error) since the last success.
    /// A non-retryable 4xx does not count because it is treated as a client error, not a congestion
    /// signal.
    pub consecutive_failures: AtomicU32,
    /// The largest payload the broker has ever rejected with 413. Once set, the controller caps the
    /// target below it. `0` means never seen.
    pub observed_payload_limit: AtomicUsize,
    /// Total request-body bytes accepted by the broker.
    pub bytes_delivered: AtomicU64,
    /// Aggregate HTTP request time in nanoseconds.
    pub request_time_ns: AtomicU64,
    /// Number of retry attempts.
    pub retries: AtomicU64,
    /// Aggregate retry backoff in nanoseconds.
    pub backoff_ns: AtomicU64,
    /// Aggregate wait while enqueuing work for broker workers.
    pub queue_wait_ns: AtomicU64,
}

impl BrokerMetrics {
    /// Records a successful request: folds its latency into the EWMA (α = 0.25, seeded from the
    /// first sample) and resets the failure streak.
    pub fn record_success(&self, latency_ms: u64) {
        let previous = self.avg_latency_ms.load(Ordering::Relaxed);
        let next = if previous == 0 {
            latency_ms
        } else {
            previous.saturating_mul(3).saturating_add(latency_ms) / 4
        };
        self.avg_latency_ms.store(next, Ordering::Relaxed);
        self.consecutive_successes.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    /// Records a failed request: extends the failure streak and resets the success streak.
    pub fn record_failure(&self) {
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        self.consecutive_successes.store(0, Ordering::Relaxed);
    }

    /// Folds a batch's observed per-entity size into the EWMA (same shape as `record_success`), so a
    /// single outlier batch cannot collapse the next batch size to the minimum.
    pub fn record_entity_size(&self, payload_bytes: usize, batch_len: usize) {
        if batch_len == 0 {
            return;
        }
        let observed = payload_bytes / batch_len;
        let previous = self.avg_entity_size_bytes.load(Ordering::Relaxed);
        let next = if previous == 0 {
            observed
        } else {
            previous.saturating_mul(3).saturating_add(observed) / 4
        };
        self.avg_entity_size_bytes.store(next, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::metrics::BrokerMetrics;
    use std::sync::atomic::Ordering;

    #[test]
    fn record_success_seeds_the_ewma_and_resets_the_failure_streak() {
        let metrics = BrokerMetrics::default();
        metrics.record_failure();
        metrics.record_failure();
        assert_eq!(metrics.consecutive_failures.load(Ordering::Relaxed), 2);

        metrics.record_success(800);
        assert_eq!(metrics.avg_latency_ms.load(Ordering::Relaxed), 800);
        assert_eq!(metrics.consecutive_failures.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.consecutive_successes.load(Ordering::Relaxed), 1);

        // Second sample: EWMA α = 0.25 → (3 * 800 + 400) / 4 = 700.
        metrics.record_success(400);
        assert_eq!(metrics.avg_latency_ms.load(Ordering::Relaxed), 700);
        assert_eq!(metrics.consecutive_successes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn record_entity_size_seeds_the_ewma_from_the_first_sample() {
        let metrics = BrokerMetrics::default();
        metrics.record_entity_size(2_000, 2);
        assert_eq!(metrics.avg_entity_size_bytes.load(Ordering::Relaxed), 1_000);

        // Second sample: (3 * 1000 + 2000) / 4 = 1_250.
        metrics.record_entity_size(2_000, 1);
        assert_eq!(metrics.avg_entity_size_bytes.load(Ordering::Relaxed), 1_250);
    }

    #[test]
    fn record_entity_size_ignores_an_empty_batch() {
        let metrics = BrokerMetrics::default();
        metrics.record_entity_size(10_000, 0);
        assert_eq!(metrics.avg_entity_size_bytes.load(Ordering::Relaxed), 0);
    }
}

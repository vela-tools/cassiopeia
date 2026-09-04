use crate::broker::{metrics::BrokerMetrics, tuning::BrokerTuning};
use std::sync::atomic::Ordering;

/// The AIMD congestion controller: reads the worker-emitted metrics and nudges the target payload
/// size by adjusting it in either direction.
///
/// A failure streak triggers a multiplicative shrink (× 0.5); a slow-latency EWMA triggers a gentler
/// multiplicative shrink (× 0.75); a sustained fast-success streak triggers a multiplicative grow
/// (× 1.1). Inside the latency dead band with no streak signal the target holds steady, so it does
/// not oscillate. The result is clamped to the tuning bounds and to any observed 413 rejection size.
pub fn adjust_target_bytes(current: usize, metrics: &BrokerMetrics, tuning: &BrokerTuning) -> usize {
    let failures = metrics.consecutive_failures.load(Ordering::Relaxed);
    let latency = metrics.avg_latency_ms.load(Ordering::Relaxed);
    let successes = metrics.consecutive_successes.load(Ordering::Relaxed);

    let mut target = current;
    if failures >= tuning.shrink_failure_threshold {
        target /= 2;
        // Clear the streak so the target is not halved again on every flush until the next success.
        metrics.consecutive_failures.store(0, Ordering::Relaxed);
    } else if latency > tuning.slow_latency_ms {
        target = target.saturating_mul(3) / 4;
    } else if successes >= tuning.grow_success_threshold && latency > 0 && latency < tuning.fast_latency_ms {
        target = target.saturating_mul(11) / 10;
        // Reset the success counter so growth is bounded rather than compounding on every flush.
        metrics.consecutive_successes.store(0, Ordering::Relaxed);
    }

    // The effective ceiling is the smaller of the tuning maximum and 80% of any payload the broker
    // has rejected with 413, so the controller never grows back into a known-oversize range.
    let observed = metrics.observed_payload_limit.load(Ordering::Relaxed);
    let upper = if observed > 0 {
        tuning.max_payload_bytes.min(observed * 4 / 5)
    } else {
        tuning.max_payload_bytes
    };
    let upper = upper.max(tuning.min_payload_bytes);

    target.clamp(tuning.min_payload_bytes, upper)
}

#[cfg(test)]
mod tests {
    use crate::broker::{controller::adjust_target_bytes, metrics::BrokerMetrics, tuning::BrokerTuning};
    use std::sync::atomic::Ordering;

    #[test]
    fn a_failure_streak_shrinks_the_target_aggressively_then_clears_the_streak() {
        let metrics = BrokerMetrics::default();
        let tuning = BrokerTuning::default();
        let start = tuning.initial_payload_bytes;

        for _ in 0..tuning.shrink_failure_threshold {
            metrics.record_failure();
        }
        let next = adjust_target_bytes(start, &metrics, &tuning);
        assert!(next < start);
        assert!(next >= start / 2 - 1 && next <= start / 2 + 1);

        // The streak was cleared, so a subsequent call with no new failures must not shrink again.
        let next_again = adjust_target_bytes(next, &metrics, &tuning);
        assert_eq!(next_again, next);
    }

    #[test]
    fn slow_latency_shrinks_the_target_gently() {
        let metrics = BrokerMetrics::default();
        let tuning = BrokerTuning::default();
        let start = tuning.initial_payload_bytes;

        metrics.avg_latency_ms.store(tuning.slow_latency_ms + 1_000, Ordering::Relaxed);
        let next = adjust_target_bytes(start, &metrics, &tuning);

        let expected = start * 3 / 4;
        assert!(next >= expected - 1 && next <= expected + 1);
    }

    #[test]
    fn a_sustained_fast_success_streak_grows_the_target_then_clears_the_streak() {
        let metrics = BrokerMetrics::default();
        let tuning = BrokerTuning::default();
        let start = tuning.initial_payload_bytes;

        metrics.avg_latency_ms.store(tuning.fast_latency_ms / 2, Ordering::Relaxed);
        metrics.consecutive_successes.store(tuning.grow_success_threshold, Ordering::Relaxed);

        let next = adjust_target_bytes(start, &metrics, &tuning);
        assert!(next > start);

        let next_again = adjust_target_bytes(next, &metrics, &tuning);
        assert_eq!(next_again, next);
    }

    #[test]
    fn the_latency_dead_band_holds_the_target_steady() {
        let metrics = BrokerMetrics::default();
        let tuning = BrokerTuning::default();
        let start = tuning.initial_payload_bytes;

        metrics
            .avg_latency_ms
            .store(u64::midpoint(tuning.fast_latency_ms, tuning.slow_latency_ms), Ordering::Relaxed);
        metrics.consecutive_successes.store(1, Ordering::Relaxed);

        assert_eq!(adjust_target_bytes(start, &metrics, &tuning), start);
    }

    #[test]
    fn the_target_never_leaves_the_tuning_bounds() {
        let metrics = BrokerMetrics::default();
        let tuning = BrokerTuning::default();

        for _ in 0..20 {
            for _ in 0..tuning.shrink_failure_threshold {
                metrics.record_failure();
            }
            assert_eq!(adjust_target_bytes(tuning.min_payload_bytes, &metrics, &tuning), tuning.min_payload_bytes);
        }

        for _ in 0..20 {
            metrics.avg_latency_ms.store(tuning.fast_latency_ms / 2, Ordering::Relaxed);
            metrics.consecutive_successes.store(tuning.grow_success_threshold, Ordering::Relaxed);
            assert_eq!(adjust_target_bytes(tuning.max_payload_bytes, &metrics, &tuning), tuning.max_payload_bytes);
        }
    }

    #[test]
    fn the_batch_size_recovers_after_a_single_large_entity_batch() {
        // Steady-state batches of 200 entities averaging 1 KiB each, plus one batch containing a
        // large outlier (payload ≈ 699 KiB). Without EWMA smoothing the next batch size would
        // collapse far below steady-state.
        let metrics = BrokerMetrics::default();
        let tuning = BrokerTuning::default();
        let target = tuning.initial_payload_bytes;
        let samples: [(usize, usize); 6] = [(200_000, 200), (200_000, 200), (200_000, 200), (699_000, 200), (200_000, 200), (200_000, 200)];

        let mut batch_sizes = Vec::new();
        for (payload, batch_len) in samples {
            metrics.record_entity_size(payload, batch_len);
            let avg = metrics.avg_entity_size_bytes.load(Ordering::Relaxed);
            batch_sizes.push((target / avg).clamp(tuning.min_batch_size, tuning.max_batch_size));
        }

        let post_spike_min = batch_sizes[3..].iter().min().copied().unwrap();
        assert!(post_spike_min > tuning.min_batch_size * 4, "EWMA over-corrected: {batch_sizes:?}");
        assert_eq!(batch_sizes[0], target / 1_000);
        assert_eq!(batch_sizes[2], target / 1_000);
    }
}

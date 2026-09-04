/// Default minimum / maximum entity counts per batch. These wrap the dynamic payload target so a
/// single very large or very small entity cannot push batch sizes outside a sensible range.
const DEFAULT_MIN_BATCH_SIZE: usize = 10;
const DEFAULT_MAX_BATCH_SIZE: usize = 10_000;

/// Default bounds on the dynamic target payload size, in bytes on the wire.
const DEFAULT_MIN_PAYLOAD_BYTES: usize = 16 * 1024;
const DEFAULT_MAX_PAYLOAD_BYTES: usize = 512 * 1024;
const DEFAULT_INITIAL_PAYLOAD_BYTES: usize = 128 * 1024;

/// Latency dead band. Below `fast`, growing is allowed; above `slow`, shrinking fires. A request
/// landing inside the band leaves the target alone.
const DEFAULT_FAST_LATENCY_MS: u64 = 500;
const DEFAULT_SLOW_LATENCY_MS: u64 = 5_000;

/// Streak thresholds for the AIMD adjustments.
const DEFAULT_GROW_SUCCESS_THRESHOLD: u32 = 10;
const DEFAULT_SHRINK_FAILURE_THRESHOLD: u32 = 2;

/// The tunable knobs of the adaptive target-payload controller.
///
/// The defaults are conservative; ops override them via
/// [`BrokerWriterConfig::with_tuning`](crate::broker::config::BrokerWriterConfig::with_tuning).
#[derive(Debug, Clone)]
pub struct BrokerTuning {
    /// The smallest entity count a batch may shrink to.
    pub min_batch_size: usize,
    /// The largest entity count a batch may grow to.
    pub max_batch_size: usize,
    /// The floor on the dynamic target payload size, in bytes.
    pub min_payload_bytes: usize,
    /// The ceiling on the dynamic target payload size, in bytes.
    pub max_payload_bytes: usize,
    /// The target payload size the run starts at, in bytes.
    pub initial_payload_bytes: usize,
    /// Latency at or below which growing the target is allowed, in milliseconds.
    pub fast_latency_ms: u64,
    /// Latency above which the target is shrunk, in milliseconds.
    pub slow_latency_ms: u64,
    /// How many consecutive fast successes trigger a grow.
    pub grow_success_threshold: u32,
    /// How many consecutive failures trigger an aggressive shrink.
    pub shrink_failure_threshold: u32,
}

impl Default for BrokerTuning {
    fn default() -> BrokerTuning {
        BrokerTuning {
            min_batch_size: DEFAULT_MIN_BATCH_SIZE,
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            min_payload_bytes: DEFAULT_MIN_PAYLOAD_BYTES,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
            initial_payload_bytes: DEFAULT_INITIAL_PAYLOAD_BYTES,
            fast_latency_ms: DEFAULT_FAST_LATENCY_MS,
            slow_latency_ms: DEFAULT_SLOW_LATENCY_MS,
            grow_success_threshold: DEFAULT_GROW_SUCCESS_THRESHOLD,
            shrink_failure_threshold: DEFAULT_SHRINK_FAILURE_THRESHOLD,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::tuning::BrokerTuning;

    #[test]
    fn the_defaults_are_internally_consistent() {
        let tuning = BrokerTuning::default();

        assert!(tuning.min_batch_size <= tuning.max_batch_size);
        assert!(tuning.min_payload_bytes <= tuning.max_payload_bytes);
        assert!((tuning.min_payload_bytes..=tuning.max_payload_bytes).contains(&tuning.initial_payload_bytes));
        assert!(tuning.fast_latency_ms < tuning.slow_latency_ms);
    }
}

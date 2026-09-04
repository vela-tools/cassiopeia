use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread::sleep,
    time::Duration,
};

/// The ceiling on the backoff delay between retries.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// The slice a backoff sleeps in, so shutdown is observed promptly mid-wait.
const SLEEP_SLICE: Duration = Duration::from_millis(100);

/// Why an interruptible backoff sleep returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffSleep {
    /// The full backoff elapsed.
    Completed,
    /// Shutdown was observed before the backoff elapsed.
    ShutdownRequested,
}

/// Exponential backoff: 1s, 2s, 4s, …, capped at 30s. `attempt` is the 1-based retry count.
#[must_use]
pub fn backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(30);
    let seconds = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
    Duration::from_secs(seconds).min(MAX_BACKOFF)
}

/// Sleeps in short increments, returning early if shutdown is observed mid-sleep.
pub fn sleep_or_shutdown(duration: Duration, shutdown: &AtomicBool) -> BackoffSleep {
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if shutdown.load(Ordering::Acquire) {
            return BackoffSleep::ShutdownRequested;
        }
        let slice = remaining.min(SLEEP_SLICE);
        sleep(slice);
        remaining = remaining.saturating_sub(slice);
    }
    BackoffSleep::Completed
}

#[cfg(test)]
mod tests {
    use crate::broker::backoff::{BackoffSleep, backoff_delay, sleep_or_shutdown};
    use std::{sync::atomic::AtomicBool, time::Duration};

    #[test]
    fn backoff_grows_then_caps_at_thirty_seconds() {
        assert_eq!(backoff_delay(1), Duration::from_secs(1));
        assert_eq!(backoff_delay(2), Duration::from_secs(2));
        assert_eq!(backoff_delay(6), Duration::from_secs(30));
        assert_eq!(backoff_delay(u32::MAX), Duration::from_secs(30));
    }

    #[test]
    fn a_requested_shutdown_ends_the_wait_at_once() {
        let shutdown = AtomicBool::new(true);

        assert_eq!(sleep_or_shutdown(Duration::from_secs(30), &shutdown), BackoffSleep::ShutdownRequested);
    }

    #[test]
    fn a_zero_backoff_completes_without_sleeping() {
        let shutdown = AtomicBool::new(false);

        assert_eq!(sleep_or_shutdown(Duration::ZERO, &shutdown), BackoffSleep::Completed);
    }
}

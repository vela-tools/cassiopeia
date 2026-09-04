use crate::wait::Wait;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread::sleep,
    time::Duration,
};

/// How often the sleep wakes to re-check the shutdown flag.
const TICK: Duration = Duration::from_millis(250);

/// Why an [`interruptible_sleep`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepOutcome {
    /// The full requested duration elapsed.
    Elapsed,
    /// Shutdown was requested before the duration elapsed.
    Interrupted,
}

/// Sleeps for `duration`, waking every [`TICK`] to observe `shutdown` so a long wait still stops
/// promptly when the pipeline is asked to shut down.
///
/// Public because the pipeline's retry backoff waits the same interruptible way between attempts,
/// reusing this primitive rather than duplicating it.
pub fn interruptible_sleep(wait: Wait, shutdown: &AtomicBool) -> SleepOutcome {
    let mut remaining = wait.duration();

    while remaining > Duration::ZERO {
        if shutdown.load(Ordering::Relaxed) {
            return SleepOutcome::Interrupted;
        }

        let step = remaining.min(TICK);
        sleep(step);
        remaining = remaining.saturating_sub(step);
    }

    if shutdown.load(Ordering::Relaxed) {
        SleepOutcome::Interrupted
    } else {
        SleepOutcome::Elapsed
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        sleep::{SleepOutcome, interruptible_sleep},
        wait::Wait,
    };
    use std::{sync::atomic::AtomicBool, time::Duration};

    #[test]
    fn a_short_sleep_with_no_shutdown_elapses_fully() {
        let shutdown = AtomicBool::new(false);

        assert_eq!(interruptible_sleep(Wait::new(Duration::from_millis(100)), &shutdown), SleepOutcome::Elapsed);
    }

    #[test]
    fn a_sleep_started_under_shutdown_is_interrupted() {
        let shutdown = AtomicBool::new(true);

        assert_eq!(interruptible_sleep(Wait::new(Duration::from_secs(10)), &shutdown), SleepOutcome::Interrupted);
    }
}

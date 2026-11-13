use cassiopeia_common::http_retry::backoff::{BackoffSleep, sleep_or_shutdown};
use cassiopeia_sensorthings::settings::request_delay::RequestDelay;
use std::{sync::atomic::AtomicBool, time::Instant};

/// Spaces the requests of one cycle at least `delay` apart, so a read spares its server a burst.
#[derive(Debug)]
pub(crate) struct RequestPacing {
    /// The least time between two requests; none when `None`.
    delay: Option<RequestDelay>,
    /// When the cycle's previous request was sent; `None` before its first.
    previous: Option<Instant>,
}

impl RequestPacing {
    /// Pacing for a cycle that has sent no request yet.
    pub(crate) const fn new(delay: Option<RequestDelay>) -> RequestPacing {
        RequestPacing { delay, previous: None }
    }

    /// Waits until `delay` has passed since the cycle's previous request, if it sent one, and counts
    /// the next request as sent now.
    ///
    /// Returns [`BackoffSleep::ShutdownRequested`] when `shutdown` cut the wait short.
    pub(crate) fn wait_turn(&mut self, shutdown: &AtomicBool) -> BackoffSleep {
        let outcome = match (self.delay, self.previous) {
            (Some(delay), Some(previous)) => sleep_or_shutdown(delay.get().saturating_sub(previous.elapsed()), shutdown),
            (Some(_) | None, None) | (None, Some(_)) => BackoffSleep::Completed,
        };
        self.previous = Some(Instant::now());
        outcome
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::request_pacing::RequestPacing;
    use cassiopeia_common::http_retry::backoff::BackoffSleep;
    use cassiopeia_sensorthings::settings::request_delay::RequestDelay;
    use pretty_assertions::assert_eq;
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    static RUNNING: AtomicBool = AtomicBool::new(false);
    static STOPPING: AtomicBool = AtomicBool::new(true);

    fn pacing(delay: Duration) -> RequestPacing {
        RequestPacing::new(Some(RequestDelay::try_from(delay).unwrap()))
    }

    #[test]
    fn the_first_request_does_not_wait() {
        let started = Instant::now();

        assert_eq!(pacing(Duration::from_secs(5)).wait_turn(&RUNNING), BackoffSleep::Completed);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_later_request_waits_the_delay() {
        let mut pacing = pacing(Duration::from_millis(50));
        pacing.wait_turn(&RUNNING);
        let started = Instant::now();

        pacing.wait_turn(&RUNNING);

        assert!(started.elapsed() >= Duration::from_millis(40), "waited {:?}", started.elapsed());
    }

    #[test]
    fn a_stop_cuts_the_wait_short() {
        let mut pacing = pacing(Duration::from_secs(5));
        pacing.wait_turn(&STOPPING);

        assert_eq!(pacing.wait_turn(&STOPPING), BackoffSleep::ShutdownRequested);
    }

    #[test]
    fn without_a_delay_no_request_waits() {
        let mut pacing = RequestPacing::new(None);
        pacing.wait_turn(&RUNNING);
        let started = Instant::now();

        pacing.wait_turn(&RUNNING);

        assert!(started.elapsed() < Duration::from_millis(40));
    }
}

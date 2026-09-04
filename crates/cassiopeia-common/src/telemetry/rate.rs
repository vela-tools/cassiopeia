//! Rolling-window throughput measurement over fixed one-second buckets.

use num_traits::ToPrimitive;
use std::{cmp, num::NonZeroU32, time::Duration};

/// The largest live-rate window, in seconds. It bounds the per-tracker bucket allocation and, because
/// the bucket vector is sized to exactly the window, keeps `second % window` inside the vector.
const MAX_WINDOW_SECONDS: u32 = 3600;

/// A validated rolling-rate window in whole seconds.
///
/// The window is both the live-rate horizon and the bucket count. Sizing the bucket vector to the
/// window means the `second % window` index used by [`RateTracker::record_completed_at`] is always in
/// range, so no window value can trigger an out-of-bounds panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateWindow(NonZeroU32);

impl RateWindow {
    /// Builds a window of `seconds`, clamped to `[1, MAX_WINDOW_SECONDS]`.
    ///
    /// A zero window rounds up to one second; anything above the maximum is capped so a caller cannot
    /// request an unbounded bucket allocation.
    #[must_use]
    pub const fn seconds(seconds: u32) -> RateWindow {
        let clamped = if seconds == 0 {
            1
        } else if seconds > MAX_WINDOW_SECONDS {
            MAX_WINDOW_SECONDS
        } else {
            seconds
        };
        match NonZeroU32::new(clamped) {
            Some(value) => RateWindow(value),
            // `clamped` is at least one, so this arm is unreachable; it keeps the constructor total
            // without an `unwrap` on a hot path.
            None => RateWindow(NonZeroU32::MIN),
        }
    }

    /// Returns the window length in seconds.
    #[must_use]
    const fn seconds_u64(self) -> u64 {
        self.0.get() as u64
    }
}

impl Default for RateWindow {
    fn default() -> RateWindow {
        RateWindow::seconds(20)
    }
}

/// A fixed one-second bucket rolling rate calculator.
///
/// The tracker records completions only. The moment a stage first received work is kept outside it,
/// in a lock-free atomic on the stage state, so a stage's hot receive path never takes the tracker's
/// lock; [`RateTracker::snapshot_at`] is handed that instant as the average-throughput denominator.
#[derive(Debug, Clone)]
pub struct RateTracker {
    window: u64,
    buckets: Vec<RateBucket>,
    first_completion: Option<Duration>,
    last_completion: Option<Duration>,
    total: u64,
}

/// One second's completed-item tally in the rolling window.
#[derive(Debug, Clone, Copy, Default)]
pub struct RateBucket {
    second: u64,
    count: u64,
}

/// The three rates intentionally use different denominators.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RateSnapshot {
    pub live_throughput: f64,
    pub average_throughput: f64,
    pub service_throughput: f64,
}

impl RateTracker {
    /// Creates a tracker with the given live window, sizing one bucket per window second.
    #[must_use]
    pub fn new(window: RateWindow) -> RateTracker {
        let window = window.seconds_u64();
        RateTracker {
            window,
            buckets: vec![RateBucket::default(); usize::try_from(window).unwrap_or(MAX_WINDOW_SECONDS as usize)],
            first_completion: None,
            last_completion: None,
            total: 0,
        }
    }

    /// Records completed items at an explicit monotonic timestamp.
    pub fn record_completed_at(&mut self, timestamp: Duration, count: u64) {
        let second = timestamp.as_secs();
        self.first_completion.get_or_insert(timestamp);
        self.last_completion = Some(timestamp);
        // `buckets.len() == window`, so this index is always valid.
        let index = usize::try_from(second % self.window).unwrap_or(0);
        let bucket = &mut self.buckets[index];
        if bucket.second != second {
            *bucket = RateBucket { second, count: 0 };
        }
        bucket.count = bucket.count.saturating_add(count);
        self.total = self.total.saturating_add(count);
    }

    /// Returns rates at an explicit timestamp and active service duration.
    ///
    /// `first_input` is the moment the stage first received work, kept outside the tracker; it is the
    /// denominator of the average throughput and `None` until the stage has received anything.
    #[must_use]
    pub fn snapshot_at(&self, timestamp: Duration, service_time: Duration, first_input: Option<Duration>) -> RateSnapshot {
        let now = timestamp.as_secs();
        let live_count = self
            .buckets
            .iter()
            .filter(|bucket| bucket.count > 0 && now.saturating_sub(bucket.second) < self.window)
            .map(|bucket| bucket.count)
            .sum::<u64>();
        let window_start = timestamp.saturating_sub(Duration::from_secs(self.window));
        let live_elapsed = self
            .first_completion
            .map_or(0.0, |first| timestamp.saturating_sub(cmp::max(first, window_start)).as_secs_f64());
        let average_elapsed = match (first_input, self.last_completion) {
            (Some(first), Some(last)) if last > first => last.saturating_sub(first).as_secs_f64(),
            _ => 0.0,
        };
        RateSnapshot {
            live_throughput: per_second(live_count, live_elapsed),
            average_throughput: per_second(self.total_completed(), average_elapsed),
            service_throughput: per_second(self.total_completed(), service_time.as_secs_f64()),
        }
    }

    #[must_use]
    pub const fn total_completed(&self) -> u64 {
        self.total
    }
}

impl Default for RateTracker {
    fn default() -> Self {
        Self::new(RateWindow::default())
    }
}

pub(super) fn per_second(count: u64, seconds: f64) -> f64 {
    if seconds > 0.0 {
        count.to_f64().map_or(0.0, |value| value / seconds)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use crate::telemetry::rate::{RateTracker, RateWindow};
    use std::time::Duration;

    #[test]
    fn rolling_window_uses_exact_one_second_buckets() {
        let mut tracker = RateTracker::new(RateWindow::seconds(3));
        tracker.record_completed_at(Duration::from_secs(1), 10);
        tracker.record_completed_at(Duration::from_secs(2), 20);
        tracker.record_completed_at(Duration::from_secs(4), 40);
        let rates = tracker.snapshot_at(Duration::from_secs(4), Duration::from_secs(2), Some(Duration::from_secs(1)));
        assert_eq!(tracker.total_completed(), 70);
        assert!((rates.live_throughput - 20.0).abs() < f64::EPSILON);
        assert!((rates.service_throughput - 35.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_average_throughput_spans_the_supplied_first_input_to_the_last_completion() {
        let mut tracker = RateTracker::new(RateWindow::seconds(60));
        tracker.record_completed_at(Duration::from_secs(4), 30);
        tracker.record_completed_at(Duration::from_secs(6), 30);

        let rates = tracker.snapshot_at(Duration::from_secs(6), Duration::ZERO, Some(Duration::from_secs(2)));
        assert!((rates.average_throughput - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_stage_that_never_received_anything_reports_no_average_throughput() {
        let mut tracker = RateTracker::new(RateWindow::seconds(60));
        tracker.record_completed_at(Duration::from_secs(1), 5);

        let rates = tracker.snapshot_at(Duration::from_secs(2), Duration::ZERO, None);
        assert!((rates.average_throughput - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn an_out_of_range_window_is_clamped_rather_than_panicking() {
        assert_eq!(RateWindow::seconds(0), RateWindow::seconds(1));
        assert_eq!(RateWindow::seconds(1_000_000), RateWindow::seconds(3600));
    }

    #[test]
    fn a_timestamp_far_beyond_the_window_does_not_index_out_of_bounds() {
        // The bucket index is `second % window`, so it stays inside the bucket vector no matter how
        // large `second` grows relative to the window.
        let mut tracker = RateTracker::new(RateWindow::seconds(2000));
        tracker.record_completed_at(Duration::from_secs(1_999_999), 5);
        assert_eq!(tracker.total_completed(), 5);
    }
}

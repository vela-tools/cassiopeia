//! Duration/nanosecond conversions shared by the atomic timing counters.
//!
//! Stage and channel metrics accumulate elapsed time as `u64` nanoseconds inside atomics, then
//! rebuild a [`Duration`] when snapshotting. Both directions saturate rather than panic, so a run
//! that somehow accumulates more than `u64::MAX` nanoseconds still produces a snapshot.

use std::time::Duration;

/// Converts a duration to whole nanoseconds, saturating at [`u64::MAX`].
pub(crate) fn to_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// Rebuilds a duration from whole nanoseconds.
pub(crate) const fn from_nanos(nanos: u64) -> Duration {
    Duration::from_nanos(nanos)
}

#[cfg(test)]
mod tests {
    use crate::telemetry::nanos::{from_nanos, to_nanos};
    use std::time::Duration;

    #[test]
    fn a_sub_max_duration_round_trips_through_nanoseconds() {
        let duration = Duration::from_millis(1250);
        assert_eq!(from_nanos(to_nanos(duration)), duration);
    }

    #[test]
    fn an_overflowing_duration_saturates_rather_than_panicking() {
        assert_eq!(to_nanos(Duration::MAX), u64::MAX);
    }
}

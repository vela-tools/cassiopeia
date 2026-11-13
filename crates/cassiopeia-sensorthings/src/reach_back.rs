use chrono::{DateTime, TimeDelta, Utc};
use std::time::Duration;

/// The instant `length` before `start`, or the earliest instant there is when `length` reaches back
/// further.
///
/// A read reaches back from an instant by a span its settings state: a range's `last` from the cycle
/// start, and a lookback from a data stream's high-water mark.
#[must_use]
pub fn reach_back(start: DateTime<Utc>, length: Duration) -> DateTime<Utc> {
    TimeDelta::from_std(length)
        .ok()
        .and_then(|length| start.checked_sub_signed(length))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

#[cfg(test)]
mod tests {
    use crate::reach_back::reach_back;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    #[test]
    fn a_span_is_taken_off_the_start() {
        assert_eq!(
            reach_back(instant("2026-11-12T10:00:00Z"), Duration::from_secs(3600)),
            instant("2026-11-12T09:00:00Z")
        );
    }

    #[test]
    fn a_span_past_the_earliest_instant_ends_at_it() {
        assert_eq!(reach_back(instant("2026-11-12T10:00:00Z"), Duration::MAX), DateTime::<Utc>::MIN_UTC);
        assert_eq!(reach_back(DateTime::<Utc>::MIN_UTC, Duration::from_secs(1)), DateTime::<Utc>::MIN_UTC);
    }
}

use chrono::{DateTime, SubsecRound, Utc};
use derive_more::Display;

/// The instant one scheduled run started, taken once before its first attempt.
///
/// Every attempt of a run builds its requests from this one instant, so a retried run asks a server
/// for exactly what its first attempt asked for, and records the same collection time. It is held to
/// the second: a request and a log line then name it the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
#[display("{}", _0.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))]
pub struct CycleStart(DateTime<Utc>);

impl CycleStart {
    /// The current instant, truncated to the second.
    #[must_use]
    pub fn now() -> CycleStart {
        CycleStart(Utc::now().trunc_subsecs(0))
    }

    /// A run that started at `instant`, truncated to the second.
    #[must_use]
    pub fn at(instant: DateTime<Utc>) -> CycleStart {
        CycleStart(instant.trunc_subsecs(0))
    }

    /// The instant the run started.
    #[must_use]
    pub const fn instant(self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::cycle_start::CycleStart;
    use chrono::{DateTime, Timelike};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_cycle_start_is_held_to_the_second() {
        let start = CycleStart::at(DateTime::parse_from_rfc3339("2026-11-12T10:00:00.750Z").unwrap().to_utc());

        assert_eq!(start.to_string(), "2026-11-12T10:00:00Z");
        assert_eq!(start.instant().nanosecond(), 0);
    }

    #[test]
    fn the_current_cycle_start_has_no_fraction() {
        assert_eq!(CycleStart::now().instant().nanosecond(), 0);
    }
}

use cassiopeia_manifest::schedule::{time_of_day::TimeOfDay, trigger::Trigger};
use chrono::{Local, TimeDelta, Timelike, Utc};
use derive_more::Deref;
use rand::RngExt;
use std::time::Duration;

/// A length of time the scheduler waits before firing the next run.
///
/// A newtype over [`Duration`] so a computed inter-run delay cannot be confused with an unrelated
/// duration (a timeout, an elapsed span). Derefs to the inner [`Duration`] for read-only queries
/// such as [`Duration::is_zero`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)]
pub struct Wait(Duration);

impl Wait {
    /// Wraps a wait duration.
    #[must_use]
    pub const fn new(duration: Duration) -> Wait {
        Wait(duration)
    }

    /// The underlying duration, for handing to the interruptible sleep.
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }
}

/// An optional upper bound on the random delay mixed into a [`Wait`].
///
/// `None`, or a zero bound, adds nothing. A non-zero bound spreads load when several deployments
/// share one schedule by staggering their runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Jitter(Option<Duration>);

impl Jitter {
    /// Wraps an optional jitter bound.
    pub(crate) const fn new(bound: Option<Duration>) -> Jitter {
        Jitter(bound)
    }
}

/// How long to wait before the next run the trigger describes.
///
/// `None` means the trigger has no further runs (an exhausted cron expression or an empty set of
/// fixed times), so the schedule is finished rather than spinning on a zero-length wait.
pub(crate) fn next_wait(trigger: &Trigger) -> Option<Wait> {
    match trigger {
        Trigger::Every(interval) => Some(Wait::new(*interval)),
        Trigger::Cron(schedule) => next_cron_wait(schedule.as_ref()),
        Trigger::At(times) => next_fixed_time_wait(times),
    }
}

/// The delay until the next instant a cron expression names, or `None` once it names no more.
fn next_cron_wait(schedule: &cron::Schedule) -> Option<Wait> {
    let now = Utc::now();

    // A negative delta means the named instant has just passed, so run immediately (zero wait)
    // rather than treating it as no-more-runs.
    schedule
        .upcoming(Utc)
        .next()
        .map(|next| Wait::new((next - now).to_std().unwrap_or(Duration::ZERO)))
}

/// The delay until the next fixed time of day, rolling over to the earliest time tomorrow once all
/// of today's times have passed. `None` when no times are given.
fn next_fixed_time_wait(times: &[TimeOfDay]) -> Option<Wait> {
    let now = Local::now().time();

    // `TimeOfDay` derefs to the underlying `NaiveTime`, which is what the comparisons need.
    let next_today = times.iter().map(|time| **time).filter(|time| *time > now).min();

    let (target, rolls_over) = match next_today {
        Some(time) => (time, false),
        None => (times.iter().map(|time| **time).min()?, true),
    };

    let seconds_now = i64::from(now.num_seconds_from_midnight());
    let seconds_target = i64::from(target.num_seconds_from_midnight());
    let mut delta = TimeDelta::seconds(seconds_target - seconds_now);
    if rolls_over {
        delta += TimeDelta::days(1);
    }

    delta.max(TimeDelta::zero()).to_std().ok().map(Wait::new)
}

/// Adds a uniform random delay of up to the jitter bound to `wait`, spreading load when several
/// deployments share one schedule. A missing or zero bound leaves the wait unchanged.
pub(crate) fn apply_jitter(wait: Wait, jitter: Jitter) -> Wait {
    match jitter.0 {
        Some(max) if !max.is_zero() => {
            let bound = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
            let extra = rand::rng().random_range(0..=bound);
            Wait::new(wait.duration() + Duration::from_millis(extra))
        }
        Some(_) | None => wait,
    }
}

#[cfg(test)]
mod tests {
    use crate::wait::{Jitter, Wait, apply_jitter, next_wait};
    use cassiopeia_manifest::schedule::{time_of_day::TimeOfDay, trigger::Trigger};
    use std::{str::FromStr, time::Duration};

    #[test]
    fn an_interval_trigger_waits_exactly_that_interval() {
        assert_eq!(next_wait(&Trigger::Every(Duration::from_secs(30))), Some(Wait::new(Duration::from_secs(30))));
    }

    #[test]
    fn a_fixed_time_trigger_waits_less_than_a_day() {
        let wait = next_wait(&Trigger::At(vec![TimeOfDay::from_str("00:00").unwrap()])).unwrap();

        assert!(wait.duration() <= Duration::from_hours(24));
    }

    #[test]
    fn a_fixed_time_trigger_with_no_times_has_no_next_run() {
        assert_eq!(next_wait(&Trigger::At(Vec::new())), None);
    }

    #[test]
    fn absent_jitter_leaves_the_wait_unchanged() {
        let wait = Wait::new(Duration::from_secs(10));

        assert_eq!(apply_jitter(wait, Jitter::new(None)), wait);
    }

    #[test]
    fn zero_jitter_leaves_the_wait_unchanged() {
        let wait = Wait::new(Duration::from_secs(10));

        assert_eq!(apply_jitter(wait, Jitter::new(Some(Duration::ZERO))), wait);
    }

    #[test]
    fn jitter_stays_within_the_stated_bound() {
        let base = Wait::new(Duration::from_secs(10));
        let max_jitter = Duration::from_millis(500);

        let jittered = apply_jitter(base, Jitter::new(Some(max_jitter)));

        assert!(jittered >= base);
        assert!(jittered <= Wait::new(base.duration() + max_jitter));
    }
}

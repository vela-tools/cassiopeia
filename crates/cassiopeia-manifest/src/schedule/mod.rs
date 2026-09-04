pub mod retry;
pub mod time_of_day;
pub mod trigger;

use crate::schedule::{retry::RetryPolicy, trigger::Trigger};
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use typed_builder::TypedBuilder;

/// When a manifest's work runs, and how often it repeats.
///
/// A manifest without a schedule runs its inputs once, so every bound here describes a repeating
/// run rather than a single one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// What makes a run start.
    #[serde(flatten)]
    #[getset(get = "pub")]
    trigger: Trigger,

    /// How many runs to perform before stopping. Unbounded when absent; zero runs is meaningless,
    /// so a stated count is non-zero by construction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    repeat: Option<NonZeroU32>,

    /// How long the schedule keeps running before stopping, written the way `humantime` reads it,
    /// such as `2h`. Unbounded when absent.
    #[serde(default, with = "humantime_serde", skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    duration: Option<Duration>,

    /// An upper bound on the random delay added before each run, which spreads load when several
    /// deployments share a schedule.
    #[serde(default, with = "humantime_serde", skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    jitter: Option<Duration>,

    /// How a failed run is retried. Not retried when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    retry: Option<RetryPolicy>,
}

impl Schedule {
    /// Assembles a schedule from its parts, or none when no trigger is present.
    ///
    /// A schedule without a trigger cannot start a run, so an absent trigger means the manifest has
    /// no schedule at all rather than an incomplete one; the remaining parts stay optional.
    #[must_use]
    pub fn assemble(
        trigger: Option<Trigger>,
        repeat: Option<NonZeroU32>,
        duration: Option<Duration>,
        jitter: Option<Duration>,
        retry: Option<RetryPolicy>,
    ) -> Option<Schedule> {
        let trigger = trigger?;

        Some(
            Schedule::builder()
                .trigger(trigger)
                .repeat(repeat)
                .duration(duration)
                .jitter(jitter)
                .retry(retry)
                .build(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::schedule::{Schedule, trigger::Trigger};
    use std::{num::NonZeroU32, time::Duration};

    #[test]
    fn reads_a_schedule_stating_only_its_trigger() {
        let schedule: Schedule = serde_json::from_str(r#"{"mode": "every", "value": "30s"}"#).unwrap();

        assert_eq!(schedule.trigger(), &Trigger::Every(Duration::from_secs(30)));
        assert_eq!(schedule.repeat(), &None);
        assert_eq!(schedule.retry(), &None);
    }

    #[test]
    fn reads_a_fully_stated_schedule() {
        let schedule: Schedule = serde_json::from_str(
            r#"{
                "mode": "every",
                "value": "30s",
                "repeat": 10,
                "duration": "2h",
                "jitter": "10s",
                "retry": {"maxAttempts": 3, "backoff": "5s"}
            }"#,
        )
        .unwrap();

        assert_eq!(schedule.repeat(), &NonZeroU32::new(10));
        assert_eq!(schedule.duration(), &Some(Duration::from_hours(2)));
        assert_eq!(schedule.jitter(), &Some(Duration::from_secs(10)));
        assert_eq!(schedule.retry().map(|retry| retry.max_attempts().get()), Some(3));
    }

    #[test]
    fn a_schedule_round_trips_through_json() {
        let schedule: Schedule = serde_json::from_str(r#"{"mode": "at", "value": ["14:00"], "repeat": 2}"#).unwrap();
        let encoded = serde_json::to_string(&schedule).unwrap();

        assert_eq!(serde_json::from_str::<Schedule>(&encoded).unwrap(), schedule);
    }

    #[test]
    fn a_schedule_without_a_trigger_is_rejected() {
        assert!(serde_json::from_str::<Schedule>(r#"{"repeat": 10}"#).is_err());
    }

    #[test]
    fn assembling_without_a_trigger_yields_no_schedule() {
        let schedule = Schedule::assemble(None, NonZeroU32::new(10), None, None, None);

        assert!(schedule.is_none());
    }

    #[test]
    fn assembling_carries_every_stated_part() {
        let schedule = Schedule::assemble(
            Some(Trigger::Every(Duration::from_secs(30))),
            NonZeroU32::new(5),
            Some(Duration::from_hours(1)),
            Some(Duration::from_secs(10)),
            None,
        )
        .unwrap();

        assert_eq!(schedule.trigger(), &Trigger::Every(Duration::from_secs(30)));
        assert_eq!(schedule.repeat(), &NonZeroU32::new(5));
        assert_eq!(schedule.duration(), &Some(Duration::from_hours(1)));
        assert_eq!(schedule.jitter(), &Some(Duration::from_secs(10)));
    }
}

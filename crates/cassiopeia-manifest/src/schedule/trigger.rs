use crate::schedule::time_of_day::TimeOfDay;
use cron::Schedule as CronSchedule;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What makes a scheduled run start.
///
/// The three forms are mutually exclusive, so the manifest names one in `mode` and gives its
/// setting in `value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "kebab-case")]
pub enum Trigger {
    /// Run once every interval, written the way `humantime` reads it, such as `30s` or `5m`.
    Every(#[serde(with = "humantime_serde")] Duration),

    /// Run on the instants a cron expression names.
    ///
    /// Boxed because a parsed cron expression is an order of magnitude larger than the other two
    /// triggers, and every value of this enum would otherwise carry that size.
    Cron(Box<CronSchedule>),

    /// Run at fixed times of day.
    At(Vec<TimeOfDay>),
}

impl Trigger {
    /// Selects `every`, then `cron`, then `at`; returns `None` when all are absent.
    ///
    /// CLI input normally supplies one mode, while this order also deterministically handles
    /// multiple populated values.
    #[must_use]
    pub fn first_present(every: Option<Duration>, cron: Option<Box<CronSchedule>>, at: Vec<TimeOfDay>) -> Option<Trigger> {
        if let Some(interval) = every {
            Some(Trigger::Every(interval))
        } else if let Some(schedule) = cron {
            Some(Trigger::Cron(schedule))
        } else if at.is_empty() {
            None
        } else {
            Some(Trigger::At(at))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schedule::{time_of_day::TimeOfDay, trigger::Trigger};
    use std::{str::FromStr, time::Duration};

    #[test]
    fn reads_an_interval_trigger() {
        let trigger: Trigger = serde_json::from_str(r#"{"mode": "every", "value": "5m"}"#).unwrap();

        assert_eq!(trigger, Trigger::Every(Duration::from_mins(5)));
    }

    #[test]
    fn reads_a_cron_trigger() {
        let trigger: Trigger = serde_json::from_str(r#"{"mode": "cron", "value": "0 0 9 * * *"}"#).unwrap();

        assert!(matches!(trigger, Trigger::Cron(_)));
    }

    #[test]
    fn reads_a_fixed_times_trigger() {
        let trigger: Trigger = serde_json::from_str(r#"{"mode": "at", "value": ["14:00", "18:30"]}"#).unwrap();

        assert_eq!(
            trigger,
            Trigger::At(vec![TimeOfDay::from_str("14:00").unwrap(), TimeOfDay::from_str("18:30").unwrap()])
        );
    }

    #[test]
    fn an_interval_trigger_round_trips_through_json() {
        let trigger = Trigger::Every(Duration::from_secs(30));
        let encoded = serde_json::to_string(&trigger).unwrap();

        assert_eq!(encoded, r#"{"mode":"every","value":"30s"}"#);
        assert_eq!(serde_json::from_str::<Trigger>(&encoded).unwrap(), trigger);
    }

    #[test]
    fn an_unrecognised_mode_is_rejected() {
        assert!(serde_json::from_str::<Trigger>(r#"{"mode": "hourly", "value": "1h"}"#).is_err());
    }

    #[test]
    fn a_malformed_cron_expression_is_rejected() {
        assert!(serde_json::from_str::<Trigger>(r#"{"mode": "cron", "value": "not a cron expression"}"#).is_err());
    }

    #[test]
    fn an_interval_argument_wins_over_the_others() {
        let trigger = Trigger::first_present(Some(Duration::from_secs(30)), None, vec![TimeOfDay::from_str("09:00").unwrap()]);

        assert_eq!(trigger, Some(Trigger::Every(Duration::from_secs(30))));
    }

    #[test]
    fn fixed_times_are_taken_when_no_earlier_argument_is_present() {
        let trigger = Trigger::first_present(None, None, vec![TimeOfDay::from_str("09:00").unwrap()]);

        assert_eq!(trigger, Some(Trigger::At(vec![TimeOfDay::from_str("09:00").unwrap()])));
    }

    #[test]
    fn no_trigger_arguments_yield_no_trigger() {
        assert_eq!(Trigger::first_present(None, None, Vec::new()), None);
    }
}

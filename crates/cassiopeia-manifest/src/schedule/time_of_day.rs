use chrono::NaiveTime;
use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The wire format for a time of day: hours and minutes, no seconds.
const FORMAT: &str = "%H:%M";

/// Raised when a manifest writes a run time that is not `HH:MM`.
#[derive(Debug, Error)]
#[error("'{value}' is not a time of day written as HH:MM")]
pub struct TimeOfDayError {
    /// The rejected input, echoed back in the error message.
    value: String,
}

/// A wall-clock time at which a scheduled run starts.
///
/// Written as `HH:MM`, which is a minute-resolution subset of what `NaiveTime` accepts; the newtype
/// exists so a manifest cannot declare a run time at second or nanosecond resolution the scheduler
/// would silently round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Display, Serialize, Deserialize)]
#[display("{}", _0.format(FORMAT))]
#[serde(try_from = "String", into = "String")]
pub struct TimeOfDay(NaiveTime);

impl FromStr for TimeOfDay {
    type Err = TimeOfDayError;

    fn from_str(value: &str) -> Result<TimeOfDay, Self::Err> {
        NaiveTime::parse_from_str(value, FORMAT)
            .map(TimeOfDay)
            .map_err(|_| TimeOfDayError { value: value.to_string() })
    }
}

impl TryFrom<String> for TimeOfDay {
    type Error = TimeOfDayError;

    fn try_from(value: String) -> Result<TimeOfDay, Self::Error> {
        value.parse()
    }
}

// No derive turns a `Display` newtype into `Into<String>`; the wire form is the `HH:MM` rendering.
impl From<TimeOfDay> for String {
    fn from(value: TimeOfDay) -> String {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::schedule::time_of_day::TimeOfDay;
    use chrono::Timelike;

    #[test]
    fn reads_a_time_written_as_hours_and_minutes() {
        let time: TimeOfDay = serde_json::from_str(r#""14:05""#).unwrap();

        assert_eq!(time.hour(), 14);
        assert_eq!(time.minute(), 5);
    }

    #[test]
    fn serializes_back_to_the_same_hours_and_minutes() {
        let time: TimeOfDay = serde_json::from_str(r#""09:30""#).unwrap();

        assert_eq!(serde_json::to_string(&time).unwrap(), r#""09:30""#);
    }

    #[test]
    fn a_time_carrying_seconds_is_rejected() {
        assert!(serde_json::from_str::<TimeOfDay>(r#""14:05:00""#).is_err());
    }

    #[test]
    fn an_hour_outside_the_day_is_rejected() {
        assert!(serde_json::from_str::<TimeOfDay>(r#""25:00""#).is_err());
    }
}

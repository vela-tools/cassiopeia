use crate::{
    instant_text::instant_text,
    read_window::{EmptyWindow, ReadWindow},
};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// The time range a range read covers, the `range` of `read: { range: { ... } }`: a span before the
/// cycle start, `last`, or a start, `from`, with an optional end, `to`.
///
/// The range holds what the manifest states, not the bounds of one cycle: each cycle computes its
/// window from the instant it started, so every attempt at one scheduled run reads the same window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RangeMembers", into = "RangeMembers")]
pub struct TimeRange(RangeForm);

/// The two forms a range is stated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RangeForm {
    /// The span of this length, longer than zero, up to the cycle start.
    Last(Duration),
    /// From `from` to `to`, which `from` is before, or to the cycle start when `to` is `None`.
    Since { from: DateTime<Utc>, to: Option<DateTime<Utc>> },
}

/// A range as a manifest writes it: `last`, or `from` with an optional `to`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeMembers {
    #[serde(default, with = "humantime_serde", skip_serializing_if = "Option::is_none")]
    last: Option<Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<DateTime<Utc>>,
}

/// Why a `range` is no time range.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimeRangeError {
    /// The range states neither `last` nor `from`.
    #[error("a `range` states `last` or `from`")]
    Unbounded,

    /// The range states both `last` and `from`.
    #[error("a `range` states `last` or `from`, not both")]
    LastAndFrom,

    /// The range states `to` without `from`.
    #[error("a `range` states `to` only beside `from`")]
    ToWithoutFrom,

    /// `last` is zero.
    #[error("`last` is longer than zero, not 0s")]
    EmptyLast,

    /// `from` is not before `to`.
    #[error("`from` is before `to`, but `from` is {} and `to` is {}", instant_text(*.from), instant_text(*.to))]
    FromNotBeforeTo {
        /// The stated `from`.
        from: DateTime<Utc>,
        /// The stated `to`.
        to: DateTime<Utc>,
    },
}

impl TimeRange {
    /// The window this range covers in the cycle that started at `cycle_start`.
    ///
    /// `last: d` covers `[cycle_start - d, cycle_start)`. `from` and `to` cover `[from, to)`, and
    /// `from` alone `[from, cycle_start)`. A `last` that reaches back past the earliest instant there
    /// is starts at that instant.
    ///
    /// # Errors
    ///
    /// Returns the [`EmptyWindow`] of the bounds when the start is not before the end, as for a
    /// `from` alone that is not before the cycle start.
    pub fn window(self, cycle_start: DateTime<Utc>) -> Result<ReadWindow, EmptyWindow> {
        match self.0 {
            RangeForm::Last(length) => ReadWindow::new(reach_back(cycle_start, length), cycle_start),
            RangeForm::Since { from, to } => ReadWindow::new(from, to.unwrap_or(cycle_start)),
        }
    }
}

/// The instant `length` before `start`, or the earliest instant there is when `length` reaches back
/// further.
fn reach_back(start: DateTime<Utc>, length: Duration) -> DateTime<Utc> {
    TimeDelta::from_std(length)
        .ok()
        .and_then(|length| start.checked_sub_signed(length))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

impl TryFrom<RangeMembers> for TimeRange {
    type Error = TimeRangeError;

    fn try_from(members: RangeMembers) -> Result<TimeRange, TimeRangeError> {
        match (members.last, members.from, members.to) {
            (None, None, None) => Err(TimeRangeError::Unbounded),
            (Some(_), Some(_), _) => Err(TimeRangeError::LastAndFrom),
            (Some(_) | None, None, Some(_)) => Err(TimeRangeError::ToWithoutFrom),
            (Some(last), None, None) if last.is_zero() => Err(TimeRangeError::EmptyLast),
            (Some(last), None, None) => Ok(TimeRange(RangeForm::Last(last))),
            (None, Some(from), Some(to)) if from >= to => Err(TimeRangeError::FromNotBeforeTo { from, to }),
            (None, Some(from), to) => Ok(TimeRange(RangeForm::Since { from, to })),
        }
    }
}

// Not derivable: each form states only its own members, which the written range leaves the others
// out of.
impl From<TimeRange> for RangeMembers {
    fn from(range: TimeRange) -> RangeMembers {
        match range.0 {
            RangeForm::Last(last) => RangeMembers {
                last: Some(last),
                from: None,
                to: None,
            },
            RangeForm::Since { from, to } => RangeMembers {
                last: None,
                from: Some(from),
                to,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::time_range::{TimeRange, TimeRangeError};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn range(json: &str) -> TimeRange {
        serde_json::from_str(json).unwrap()
    }

    /// The bounds of the window `range` covers in a cycle that started at 2026-11-12T10:00:00Z.
    fn bounds(range: TimeRange) -> (DateTime<Utc>, DateTime<Utc>) {
        let window = range.window(instant("2026-11-12T10:00:00Z")).unwrap();
        (window.from(), window.to())
    }

    #[test]
    fn a_last_range_ends_at_the_cycle_start() {
        assert_eq!(
            bounds(range(r#"{"last": "24h"}"#)),
            (instant("2026-11-11T10:00:00Z"), instant("2026-11-12T10:00:00Z"))
        );
    }

    #[test]
    fn an_absolute_range_is_its_own_bounds() {
        assert_eq!(
            bounds(range(r#"{"from": "2026-11-01T00:00:00Z", "to": "2026-11-02T00:00:00Z"}"#)),
            (instant("2026-11-01T00:00:00Z"), instant("2026-11-02T00:00:00Z"))
        );
    }

    #[test]
    fn a_range_without_an_end_ends_at_the_cycle_start() {
        assert_eq!(
            bounds(range(r#"{"from": "2026-11-01T00:00:00Z"}"#)),
            (instant("2026-11-01T00:00:00Z"), instant("2026-11-12T10:00:00Z"))
        );
    }

    #[test]
    fn a_range_from_after_the_cycle_start_is_an_empty_window() {
        let empty = range(r#"{"from": "2026-11-13T00:00:00Z"}"#)
            .window(instant("2026-11-12T10:00:00Z"))
            .unwrap_err();

        assert_eq!((empty.from(), empty.to()), (instant("2026-11-13T00:00:00Z"), instant("2026-11-12T10:00:00Z")));
    }

    #[test]
    fn a_last_range_past_the_earliest_instant_starts_at_it() {
        let window = range(r#"{"last": "500000years"}"#).window(instant("2026-11-12T10:00:00Z")).unwrap();

        assert_eq!(window.from(), DateTime::<Utc>::MIN_UTC);
    }

    #[test]
    fn an_offset_is_read_as_the_utc_instant_it_names() {
        assert_eq!(bounds(range(r#"{"from": "2026-11-01T02:00:00+02:00"}"#)).0, instant("2026-11-01T00:00:00Z"));
    }

    #[test]
    fn a_range_is_written_as_it_was_stated() {
        for json in [
            r#"{"last":"1day"}"#,
            r#"{"from":"2026-11-01T00:00:00Z"}"#,
            r#"{"from":"2026-11-01T00:00:00Z","to":"2026-11-02T00:00:00.250Z"}"#,
        ] {
            assert_eq!(serde_json::to_string(&range(json)).unwrap(), json);
        }
    }

    #[test]
    fn each_malformed_range_names_what_is_wrong() {
        for (json, refusal) in [
            ("{}", TimeRangeError::Unbounded),
            (r#"{"last": "1h", "from": "2026-11-01T00:00:00Z"}"#, TimeRangeError::LastAndFrom),
            (r#"{"to": "2026-11-02T00:00:00Z"}"#, TimeRangeError::ToWithoutFrom),
            (r#"{"last": "1h", "to": "2026-11-02T00:00:00Z"}"#, TimeRangeError::ToWithoutFrom),
            (r#"{"last": "0s"}"#, TimeRangeError::EmptyLast),
            (
                r#"{"from": "2026-11-02T00:00:00Z", "to": "2026-11-01T00:00:00Z"}"#,
                TimeRangeError::FromNotBeforeTo {
                    from: instant("2026-11-02T00:00:00Z"),
                    to: instant("2026-11-01T00:00:00Z"),
                },
            ),
        ] {
            let error = serde_json::from_str::<TimeRange>(json).unwrap_err();

            assert!(error.to_string().contains(&refusal.to_string()), "{json}: {error}");
        }
    }

    #[test]
    fn a_range_that_is_not_before_its_end_names_both_bounds() {
        let refusal = TimeRangeError::FromNotBeforeTo {
            from: instant("2026-11-02T00:00:00Z"),
            to: instant("2026-11-02T00:00:00Z"),
        };

        assert_eq!(
            refusal.to_string(),
            "`from` is before `to`, but `from` is 2026-11-02T00:00:00Z and `to` is 2026-11-02T00:00:00Z"
        );
    }

    #[test]
    fn unknown_members_and_unreadable_instants_are_refused() {
        for json in [
            r#"{"last": "1h", "until": "2026-11-02T00:00:00Z"}"#,
            r#"{"from": "2026-11-01"}"#,
            r#"{"last": "soon"}"#,
        ] {
            assert!(serde_json::from_str::<TimeRange>(json).is_err(), "{json}");
        }
    }
}

use crate::{cursor_name::CursorName, instant_text::instant_text, reach_back::reach_back};
use chrono::{DateTime, Utc};
use derive_more::Display;
use getset::{CopyGetters, Getters};
use humantime::{format_duration, parse_duration};
use humantime_serde::Serde;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::time::Duration;
use thiserror::Error;

/// How far before the cycle start a data stream without a position starts, unless `initial` says
/// otherwise.
const DEFAULT_INITIAL: Duration = Duration::from_hours(24);

/// How far behind its high-water mark a data stream is read again, unless `lookback` says otherwise.
const DEFAULT_LOOKBACK: Duration = Duration::from_hours(1);

/// The longest lookback: seven days.
const MAX_LOOKBACK: Duration = Duration::from_hours(7 * 24);

/// An incremental read, the `incremental` of `read: { incremental: { ... } }`: every reading a data
/// stream gained since the last successful cycle, resumed from the position its cursor keeps.
///
/// A data stream with a position is read again from a lookback behind its high-water mark, so a
/// reading the server uploads late is still read; one without a position starts at the initial
/// point. Either way the read ends at the cycle start.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Getters, CopyGetters)]
#[serde(deny_unknown_fields)]
pub struct IncrementalRead {
    /// The cursor that keeps the read's positions.
    #[getset(get = "pub")]
    cursor: CursorName,
    /// Where a data stream without a position starts.
    #[serde(default)]
    #[getset(get_copy = "pub")]
    initial: InitialPoint,
    /// How far behind its high-water mark each data stream is read again.
    #[serde(default)]
    #[getset(get_copy = "pub")]
    lookback: Lookback,
}

/// Where a data stream without a position starts: an instant, or a span before the cycle start.
///
/// Written as an RFC 3339 instant, such as `2026-01-01T00:00:00Z`, or as a span the way `humantime`
/// reads it, such as `7d`. Any text that is no RFC 3339 instant is read as a span. The default is 24
/// hours before the cycle start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, SmartDefault, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum InitialPoint {
    /// This instant.
    #[display("{}", instant_text(*_0))]
    Instant(DateTime<Utc>),
    /// This span, longer than zero, before the cycle start.
    #[default]
    #[display("{}", format_duration(*_0))]
    Before(#[default(DEFAULT_INITIAL)] Duration),
}

/// Why a text is no initial point.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InitialPointError {
    /// The text is neither an RFC 3339 instant nor a span.
    #[error("`initial` is an RFC 3339 instant or a span such as `24h`, not `{0}`")]
    Unreadable(Box<str>),

    /// The text is a span of zero.
    #[error("`initial` is a span longer than zero, not 0s")]
    Empty,
}

/// How far behind its high-water mark an incremental read reads each data stream again, written the
/// way `humantime` reads it, such as `1h`, and at most seven days.
///
/// A server that uploads a reading late gives it an earlier `phenomenonTime` than readings already
/// read, so only a read that reaches back that far finds it. The default is one hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault, Serialize, Deserialize)]
#[serde(try_from = "Serde<Duration>")]
pub struct Lookback(
    #[default(DEFAULT_LOOKBACK)]
    #[serde(with = "humantime_serde")]
    Duration,
);

/// Why a duration is no lookback.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("`lookback` is at most 7d, not {}", format_duration(*.0))]
pub struct LookbackError(Duration);

impl IncrementalRead {
    /// Where a data stream is read from in the cycle that started at `cycle_start`: a lookback
    /// behind `high_water`, its high-water mark, or the initial point when it has none.
    #[must_use]
    pub fn start(&self, high_water: Option<DateTime<Utc>>, cycle_start: DateTime<Utc>) -> DateTime<Utc> {
        match high_water {
            Some(high_water) => reach_back(high_water, self.lookback.get()),
            None => self.initial.at(cycle_start),
        }
    }
}

impl InitialPoint {
    /// The instant this point names in the cycle that started at `cycle_start`, or the earliest
    /// instant there is when a span reaches back further.
    #[must_use]
    pub fn at(self, cycle_start: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            InitialPoint::Instant(instant) => instant,
            InitialPoint::Before(span) => reach_back(cycle_start, span),
        }
    }
}

impl TryFrom<String> for InitialPoint {
    type Error = InitialPointError;

    fn try_from(text: String) -> Result<InitialPoint, InitialPointError> {
        if let Ok(instant) = DateTime::parse_from_rfc3339(&text) {
            return Ok(InitialPoint::Instant(instant.to_utc()));
        }
        match parse_duration(&text) {
            Ok(span) if span.is_zero() => Err(InitialPointError::Empty),
            Ok(span) => Ok(InitialPoint::Before(span)),
            Err(_) => Err(InitialPointError::Unreadable(text.into_boxed_str())),
        }
    }
}

// Not derivable: each form is written in its own notation, which `Display` writes.
impl From<InitialPoint> for String {
    fn from(point: InitialPoint) -> String {
        point.to_string()
    }
}

impl Lookback {
    /// How far behind its high-water mark a data stream is read again.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl TryFrom<Duration> for Lookback {
    type Error = LookbackError;

    fn try_from(lookback: Duration) -> Result<Lookback, LookbackError> {
        if lookback > MAX_LOOKBACK {
            return Err(LookbackError(lookback));
        }
        Ok(Lookback(lookback))
    }
}

impl TryFrom<Serde<Duration>> for Lookback {
    type Error = LookbackError;

    fn try_from(lookback: Serde<Duration>) -> Result<Lookback, LookbackError> {
        Lookback::try_from(lookback.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::incremental_read::{IncrementalRead, InitialPoint, InitialPointError, Lookback};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn read(json: &str) -> Result<IncrementalRead, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The start of the cycle every test reads in, 2026-11-12T10:00:00Z.
    fn cycle_start() -> DateTime<Utc> {
        instant("2026-11-12T10:00:00Z")
    }

    #[test]
    fn incremental_settings_default_their_initial_point_and_lookback() {
        let read = read(r#"{"cursor": "rivm-air"}"#).unwrap();

        assert_eq!(read.cursor().as_str(), "rivm-air");
        assert_eq!(read.initial().at(cycle_start()), instant("2026-11-11T10:00:00Z"));
        assert_eq!(read.lookback().get(), Duration::from_hours(1));
    }

    #[test]
    fn an_rfc_3339_initial_point_is_an_instant_and_any_other_a_duration() {
        let instant_point = read(r#"{"cursor": "a", "initial": "2026-01-01T00:00:00Z"}"#).unwrap().initial();
        let span = read(r#"{"cursor": "a", "initial": "7d"}"#).unwrap().initial();

        assert_eq!(instant_point, InitialPoint::Instant(instant("2026-01-01T00:00:00Z")));
        assert_eq!(instant_point.at(cycle_start()), instant("2026-01-01T00:00:00Z"));
        assert_eq!(span, InitialPoint::Before(Duration::from_hours(7 * 24)));
        assert_eq!(span.at(cycle_start()), instant("2026-11-05T10:00:00Z"));
    }

    #[test]
    fn a_lookback_over_seven_days_is_refused() {
        assert_eq!(
            read(r#"{"cursor": "a", "lookback": "7d"}"#).unwrap().lookback().get(),
            Duration::from_hours(7 * 24)
        );
        assert_eq!(read(r#"{"cursor": "a", "lookback": "0s"}"#).unwrap().lookback().get(), Duration::ZERO);

        let error = read(r#"{"cursor": "a", "lookback": "8d"}"#).unwrap_err();

        assert!(error.to_string().contains("`lookback` is at most 7d, not 8days"), "{error}");
    }

    #[test]
    fn an_unreadable_or_empty_initial_point_is_refused() {
        for (text, refusal) in [
            ("soon", InitialPointError::Unreadable("soon".into())),
            ("2026-01-01", InitialPointError::Unreadable("2026-01-01".into())),
            ("0s", InitialPointError::Empty),
        ] {
            assert_eq!(InitialPoint::try_from(text.to_owned()), Err(refusal), "{text}");
        }
    }

    #[test]
    fn malformed_incremental_settings_are_refused() {
        for json in [
            "{}",
            r#"{"cursor": "Rivm"}"#,
            r#"{"cursor": "a", "since": "1h"}"#,
            r#"{"cursor": "a", "initial": "soon"}"#,
            r#"{"cursor": "a", "lookback": "soon"}"#,
        ] {
            assert!(read(json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_known_stream_starts_a_lookback_behind_its_high_water_and_a_new_one_at_the_initial_point() {
        let read = read(r#"{"cursor": "a", "initial": "2026-11-01T00:00:00Z", "lookback": "30m"}"#).unwrap();

        assert_eq!(
            read.start(Some(instant("2026-11-12T09:00:00Z")), cycle_start()),
            instant("2026-11-12T08:30:00Z")
        );
        assert_eq!(read.start(None, cycle_start()), instant("2026-11-01T00:00:00Z"));
    }

    #[test]
    fn incremental_settings_round_trip_through_serialization() {
        for json in [
            r#"{"cursor":"rivm-air","initial":"1day","lookback":"1h"}"#,
            r#"{"cursor":"rivm-air","initial":"2026-01-01T00:00:00Z","lookback":"30m"}"#,
        ] {
            assert_eq!(serde_json::to_string(&read(json).unwrap()).unwrap(), json);
        }
    }

    #[test]
    fn the_defaults_are_a_day_back_and_an_hour_behind() {
        assert_eq!(InitialPoint::default(), InitialPoint::Before(Duration::from_hours(24)));
        assert_eq!(Lookback::default().get(), Duration::from_hours(1));
    }
}

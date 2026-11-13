use crate::{instant_text::instant_text, reach_back::reach_back};
use chrono::{DateTime, Utc};
use derive_more::Display;
use humantime::{format_duration, parse_duration};
use std::{str::FromStr, time::Duration};
use thiserror::Error;

/// A point in time: an instant, or a span before an instant the point is read against.
///
/// Written as an RFC 3339 instant, such as `2026-01-01T00:00:00Z`, or as a span the way `humantime`
/// reads it, such as `7d`. Any text that is no RFC 3339 instant is read as a span. Where an
/// incremental read starts a data stream without a position is a point read against the cycle start,
/// and where a cursor reset leaves a high-water mark is one read against the moment of the reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum TimePoint {
    /// This instant.
    #[display("{}", instant_text(*_0))]
    Instant(DateTime<Utc>),
    /// This span before the instant the point is read against.
    #[display("{}", format_duration(*_0))]
    Before(Duration),
}

/// Why a text is no point in time.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("an RFC 3339 instant or a span such as `24h`, not `{0}`")]
pub struct TimePointError(Box<str>);

impl TimePoint {
    /// The instant this point names when read against `reference`, or the earliest instant there is
    /// when a span reaches back further.
    #[must_use]
    pub fn at(self, reference: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            TimePoint::Instant(instant) => instant,
            TimePoint::Before(span) => reach_back(reference, span),
        }
    }
}

impl FromStr for TimePoint {
    type Err = TimePointError;

    /// Reads a point written as an RFC 3339 instant, or else as a span.
    fn from_str(text: &str) -> Result<TimePoint, TimePointError> {
        if let Ok(instant) = DateTime::parse_from_rfc3339(text) {
            return Ok(TimePoint::Instant(instant.to_utc()));
        }
        parse_duration(text).map(TimePoint::Before).map_err(|_| TimePointError(Box::from(text)))
    }
}

#[cfg(test)]
mod tests {
    use crate::time_point::{TimePoint, TimePointError};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    #[test]
    fn an_rfc_3339_text_is_an_instant_and_any_other_a_span() {
        assert_eq!("2026-01-01T02:00:00+02:00".parse(), Ok(TimePoint::Instant(instant("2026-01-01T00:00:00Z"))));
        assert_eq!("7d".parse(), Ok(TimePoint::Before(Duration::from_hours(7 * 24))));
        assert_eq!("0s".parse(), Ok(TimePoint::Before(Duration::ZERO)));
    }

    #[test]
    fn a_text_that_is_neither_is_refused_and_named() {
        for text in ["soon", "2026-01-01", ""] {
            assert_eq!(text.parse::<TimePoint>(), Err(TimePointError(Box::from(text))), "{text}");
        }
        assert_eq!(
            "soon".parse::<TimePoint>().unwrap_err().to_string(),
            "an RFC 3339 instant or a span such as `24h`, not `soon`"
        );
    }

    #[test]
    fn an_instant_names_itself_and_a_span_the_instant_that_long_before_the_reference() {
        let reference = instant("2026-11-12T10:00:00Z");

        assert_eq!(
            TimePoint::Instant(instant("2026-01-01T00:00:00Z")).at(reference),
            instant("2026-01-01T00:00:00Z")
        );
        assert_eq!(TimePoint::Before(Duration::from_hours(1)).at(reference), instant("2026-11-12T09:00:00Z"));
        assert_eq!(TimePoint::Before(Duration::MAX).at(reference), DateTime::<Utc>::MIN_UTC);
    }

    #[test]
    fn a_point_is_written_in_the_notation_it_is_read_in() {
        for text in ["2026-01-01T00:00:00Z", "1day", "1h 30m"] {
            assert_eq!(text.parse::<TimePoint>().unwrap().to_string(), text);
        }
    }
}

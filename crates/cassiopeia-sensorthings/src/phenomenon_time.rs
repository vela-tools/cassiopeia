use crate::instant_text::instant_text;
use chrono::{DateTime, SubsecRound, Utc};
use serde_json::{Map, Value};
use std::str::FromStr;
use thiserror::Error;

/// The member a served Observation carries its `phenomenonTime` in (OGC 18-088 section 8.2.7, Table
/// 18), and a served Datastream the span of its Observations' (section 8.2.4, Table 10).
pub const PHENOMENON_TIME_MEMBER: &str = "phenomenonTime";

/// How many fractional digits of a second an instant keeps: NGSI-LD times are precise to the
/// microsecond (ETSI GS CIM 009 v1.9.1 clause 4.6.3).
const FRACTION_DIGITS: u16 = 6;

/// An Observation's `phenomenonTime` and the instant it stands for (OGC 18-088 section 8.2.7,
/// Table 18).
///
/// A `phenomenonTime` is an instant or an interval of two instants. The instant it stands for is the
/// instant itself, or the end of the interval, the moment the reading was complete. It is held in
/// UTC and truncated to microseconds, the precision NGSI-LD keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhenomenonTime {
    /// The text the server sent.
    text: Box<str>,
    /// The instant the text stands for.
    instant: DateTime<Utc>,
}

/// A `phenomenonTime` that is no instant and no interval of two instants.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("`{text}` is neither an instant nor an interval of two instants")]
pub struct UnreadablePhenomenonTime {
    /// The text the server sent.
    text: Box<str>,
}

impl UnreadablePhenomenonTime {
    /// The text the server sent.
    #[must_use]
    pub const fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the failure and returns the text the server sent.
    #[must_use]
    pub fn into_text(self) -> Box<str> {
        self.text
    }
}

impl PhenomenonTime {
    /// The `phenomenonTime` `served` carries, or `None` when it carries none, or one that is no text
    /// or names no instant.
    #[must_use]
    pub fn of(served: &Map<String, Value>) -> Option<PhenomenonTime> {
        served.get(PHENOMENON_TIME_MEMBER)?.as_str()?.parse().ok()
    }

    /// The text the server sent.
    #[must_use]
    pub const fn text(&self) -> &str {
        &self.text
    }

    /// The instant the time stands for, in UTC.
    #[must_use]
    pub const fn instant(&self) -> DateTime<Utc> {
        self.instant
    }

    /// The instant the time stands for, written as [`instant_text`] writes an instant.
    #[must_use]
    pub fn instant_text(&self) -> String {
        instant_text(self.instant)
    }
}

impl FromStr for PhenomenonTime {
    type Err = UnreadablePhenomenonTime;

    fn from_str(text: &str) -> Result<PhenomenonTime, UnreadablePhenomenonTime> {
        let end = text.split_once('/').map_or(Some(text), |(start, end)| instant(start).map(|_| end));
        end.and_then(instant)
            .map(|instant| PhenomenonTime { text: text.into(), instant })
            .ok_or_else(|| UnreadablePhenomenonTime { text: text.into() })
    }
}

/// The instant an RFC 3339 date and time with a UTC offset names, in UTC, truncated to microseconds.
///
/// A text without a time or without an offset names no instant, and an ISO 8601 duration is no
/// instant either, so an interval written as a start and a duration is unreadable.
fn instant(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|instant| instant.with_timezone(&Utc).trunc_subsecs(FRACTION_DIGITS))
}

#[cfg(test)]
mod tests {
    use crate::phenomenon_time::{PhenomenonTime, UnreadablePhenomenonTime};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn instant_of(text: &str) -> String {
        text.parse::<PhenomenonTime>().unwrap().instant_text()
    }

    #[test]
    fn an_instant_phenomenon_time_is_its_own_time() {
        assert_eq!(instant_of("2026-11-12T10:00:00Z"), "2026-11-12T10:00:00Z");
    }

    #[test]
    fn an_interval_phenomenon_time_reads_its_end() {
        assert_eq!(instant_of("2026-11-12T09:00:00Z/2026-11-12T10:00:00Z"), "2026-11-12T10:00:00Z");
    }

    #[test]
    fn offsets_are_converted_to_utc() {
        assert_eq!(instant_of("2010-12-23T10:20:00.00-07:00/2010-12-23T12:20:00.00-07:00"), "2010-12-23T19:20:00Z");
        assert_eq!(instant_of("2026-11-12T12:00:00+02:00"), "2026-11-12T10:00:00Z");
    }

    #[test]
    fn fractions_are_truncated_to_microseconds() {
        assert_eq!(instant_of("2026-11-12T10:00:00.123456789Z"), "2026-11-12T10:00:00.123456Z");
    }

    #[test]
    fn a_zero_fraction_is_left_out() {
        assert_eq!(instant_of("2026-11-12T11:00:00.000Z"), "2026-11-12T11:00:00Z");
    }

    #[test]
    fn a_served_entity_s_phenomenon_time_is_read_from_its_member() {
        let served = |value: Value| -> Map<String, Value> { Map::from_iter([("phenomenonTime".to_owned(), value)]) };

        assert_eq!(
            PhenomenonTime::of(&served(json!("2026-11-12T09:00:00Z/2026-11-12T10:00:00Z"))).map(|time| time.instant_text()),
            Some("2026-11-12T10:00:00Z".to_owned())
        );
        assert_eq!(PhenomenonTime::of(&served(json!("soon"))), None);
        assert_eq!(PhenomenonTime::of(&served(json!(null))), None);
        assert_eq!(PhenomenonTime::of(&Map::new()), None);
    }

    #[test]
    fn the_text_is_kept_as_sent() {
        let time = "2026-11-12T09:00:00Z/2026-11-12T10:00:00Z".parse::<PhenomenonTime>().unwrap();

        assert_eq!(time.text(), "2026-11-12T09:00:00Z/2026-11-12T10:00:00Z");
    }

    #[test]
    fn other_phenomenon_times_are_unreadable() {
        for text in [
            "2026-11-12",
            "2026-11-12T10:00:00",
            "2026-11-12T09:00:00Z/PT1H",
            "soon",
            "soon/2026-11-12T10:00:00Z",
        ] {
            let error: UnreadablePhenomenonTime = text.parse::<PhenomenonTime>().unwrap_err();
            assert_eq!(error.text(), text);
        }
    }
}

//! The text an NGSI-LD time value is written as (ETSI GS CIM 009 v1.9.1 clause 4.6.3).
//!
//! Clause 4.6.3 spells a `DateTime` as `YYYY-MM-DDThh:mm:ssZ`, a `Date` as `YYYY-MM-DD` and a `Time`
//! as `hh:mm:ssZ`, and lets the seconds carry "one or more fractional digits, up to a maximum of
//! six". A source can be finer than that, so an instant is truncated to its microsecond before it is
//! written, which never carries into the next second. What remains is written at the precision it
//! has: no fraction for a whole second, three digits for a whole millisecond, six otherwise. Every
//! writer of a value, an `observedAt` or an `expiresAt` goes through here, so no two spell the same
//! instant differently.

use crate::value::types::TemporalValue;
use chrono::{
    DateTime,
    SubsecRound,
    Utc,
    format::{DelayedFormat, StrftimeItems},
};

/// The most fractional-second digits clause 4.6.3 admits.
const MICROSECOND_DIGITS: u16 = 6;

/// The text of `instant` as an NGSI-LD `DateTime`: `2026-10-24T19:49:09.712623Z`.
///
/// `%.f` writes no fraction, or three, six or nine digits, whichever the instant needs; after the
/// truncation nine never occurs.
#[must_use]
pub fn date_time(instant: DateTime<Utc>) -> DelayedFormat<StrftimeItems<'static>> {
    instant.trunc_subsecs(MICROSECOND_DIGITS).format("%Y-%m-%dT%H:%M:%S%.fZ")
}

/// The text of the calendar day of `instant` as an NGSI-LD `Date`: `2026-10-24`.
#[must_use]
pub fn date(instant: DateTime<Utc>) -> DelayedFormat<StrftimeItems<'static>> {
    instant.format("%Y-%m-%d")
}

/// The text of the time of day of `instant` as an NGSI-LD `Time`: `19:49:09.712623Z`.
#[must_use]
pub fn time(instant: DateTime<Utc>) -> DelayedFormat<StrftimeItems<'static>> {
    instant.trunc_subsecs(MICROSECOND_DIGITS).format("%H:%M:%S%.fZ")
}

impl TemporalValue {
    /// The text this value is written as, in the form of the data type it holds.
    #[must_use]
    pub fn text(&self) -> DelayedFormat<StrftimeItems<'static>> {
        match *self {
            TemporalValue::DateTime(instant) => date_time(instant),
            TemporalValue::Date(instant) => date(instant),
            TemporalValue::Time(instant) => time(instant),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::{
        temporal_text::{date, date_time, time},
        types::TemporalValue,
    };
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use pretty_assertions::assert_eq;

    /// The instant `2026-10-24T19:49:09Z`.
    fn whole_second() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 24, 19, 49, 9).unwrap()
    }

    #[test]
    fn a_date_time_is_written_at_the_precision_it_has() {
        assert_eq!(date_time(whole_second()).to_string(), "2026-10-24T19:49:09Z");
        assert_eq!(date_time(whole_second() + Duration::milliseconds(712)).to_string(), "2026-10-24T19:49:09.712Z");
        assert_eq!(
            date_time(whole_second() + Duration::microseconds(712_623)).to_string(),
            "2026-10-24T19:49:09.712623Z"
        );
        assert_eq!(
            date_time(whole_second() + Duration::microseconds(90)).to_string(),
            "2026-10-24T19:49:09.000090Z"
        );
    }

    #[test]
    fn a_date_time_finer_than_a_microsecond_is_truncated_without_carrying() {
        assert_eq!(
            date_time(whole_second() + Duration::nanoseconds(712_623_200)).to_string(),
            "2026-10-24T19:49:09.712623Z"
        );
        assert_eq!(
            date_time(whole_second() + Duration::nanoseconds(999_999_999)).to_string(),
            "2026-10-24T19:49:09.999999Z"
        );
        assert_eq!(
            date_time(whole_second() + Duration::nanoseconds(712_000_100)).to_string(),
            "2026-10-24T19:49:09.712Z"
        );
    }

    #[test]
    fn a_date_is_written_without_its_time_of_day() {
        assert_eq!(date(whole_second() + Duration::nanoseconds(1)).to_string(), "2026-10-24");
    }

    #[test]
    fn a_time_carries_its_fraction_and_the_utc_designator() {
        assert_eq!(time(whole_second()).to_string(), "19:49:09Z");
        assert_eq!(time(whole_second() + Duration::nanoseconds(712_623_245)).to_string(), "19:49:09.712623Z");
    }

    #[test]
    fn a_temporal_value_is_written_in_the_form_of_its_data_type() {
        let instant = whole_second() + Duration::microseconds(250_000);

        assert_eq!(TemporalValue::DateTime(instant).text().to_string(), "2026-10-24T19:49:09.250Z");
        assert_eq!(TemporalValue::Date(instant).text().to_string(), "2026-10-24");
        assert_eq!(TemporalValue::Time(instant).text().to_string(), "19:49:09.250Z");
    }
}

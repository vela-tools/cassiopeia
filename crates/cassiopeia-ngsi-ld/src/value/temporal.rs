use crate::value::{
    parsing::parse_float,
    types::{Number, TemporalValue},
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use num_traits::ToPrimitive;

/// The unit a Unix epoch number is counted in, read off its magnitude.
///
/// A source writes an epoch as seconds, milliseconds or nanoseconds without saying which, so the
/// unit is inferred: a present-day instant is about 1.7e9 seconds, 1.7e12 milliseconds or 1.7e18
/// nanoseconds, and the bands below separate those orders of magnitude. A magnitude below the
/// seconds band is a year, a count or an identifier rather than an instant.
#[derive(Clone, Copy)]
enum EpochUnit {
    Seconds,
    Milliseconds,
    Nanoseconds,
}

impl EpochUnit {
    /// The unit a whole epoch count of `whole` is in, or `None` when it is too small to be one.
    const fn of(whole: i64) -> Option<EpochUnit> {
        match whole.unsigned_abs() {
            0..200_000_000 => None,
            200_000_000..100_000_000_000 => Some(EpochUnit::Seconds),
            100_000_000_000..100_000_000_000_000 => Some(EpochUnit::Milliseconds),
            100_000_000_000_000.. => Some(EpochUnit::Nanoseconds),
        }
    }

    /// How many nanoseconds one count of this unit spans.
    const fn nanoseconds(self) -> u32 {
        match self {
            EpochUnit::Seconds => 1_000_000_000,
            EpochUnit::Milliseconds => 1_000_000,
            EpochUnit::Nanoseconds => 1,
        }
    }
}

/// One candidate spelling of a space-separated timestamp.
///
/// The two families cannot be tried through one parser: [`NaiveDateTime::parse_from_str`] refuses a
/// trailing UTC offset, and [`DateTime::parse_from_str`] requires one. Which family an input belongs
/// to is read off its bytes before either is tried, so a naive timestamp never pays for an
/// offset-bearing parse and the other way round.
#[derive(Clone, Copy)]
enum SpaceSeparated {
    /// A spelling carrying a numeric UTC offset, which is applied to reach the instant.
    Offset(&'static str),
    /// A spelling carrying no zone at all, which is read as UTC.
    Naive(&'static str),
}

impl SpaceSeparated {
    /// Reads `s` as this spelling, normalising the result to UTC.
    fn parse(self, s: &str) -> Option<DateTime<Utc>> {
        match self {
            SpaceSeparated::Offset(format) => DateTime::parse_from_str(s, format).ok().map(|dt| dt.with_timezone(&Utc)),
            SpaceSeparated::Naive(format) => NaiveDateTime::parse_from_str(s, format).ok().map(|ndt| Utc.from_utc_datetime(&ndt)),
        }
    }
}

/// The spellings a space-separated timestamp of `s`'s byte shape can have, seconds-bearing first.
///
/// Four facts decide the family, and all four are read off the bytes rather than discovered by
/// failed parses: the date separator is `/` for d/m/Y and `-` for Y-m-d, the time separator is `:`
/// or `.`, and a `+`/`-` sign or a trailing `Z` in the time half marks a UTC offset. `%.f` matches
/// the empty string, so one seconds-bearing spelling covers both a whole and a fractional second,
/// and `%#z` accepts `+00:00`, `+0000`, `+02` and `Z` alike, which is what makes an offset-bearing
/// space-separated timestamp read as the same instant as its RFC 3339 spelling.
fn space_separated_candidates(s: &str) -> &'static [SpaceSeparated] {
    let Some((date, time)) = s.split_once(' ') else {
        return &[];
    };
    let (date, time) = (date.as_bytes(), time.as_bytes());
    let zoned = time.contains(&b'+') || time.contains(&b'-') || matches!(time.last(), Some(b'Z' | b'z'));

    match (date.contains(&b'/'), time.contains(&b':'), time.contains(&b'.'), zoned) {
        (false, true, _, false) => &[SpaceSeparated::Naive("%Y-%m-%d %H:%M:%S%.f"), SpaceSeparated::Naive("%Y-%m-%d %H:%M")],
        (false, true, _, true) => &[SpaceSeparated::Offset("%Y-%m-%d %H:%M:%S%.f%#z"), SpaceSeparated::Offset("%Y-%m-%d %H:%M%#z")],
        (false, false, true, _) => &[SpaceSeparated::Naive("%Y-%m-%d %H.%M.%S")],
        (true, true, _, false) => &[SpaceSeparated::Naive("%d/%m/%Y %H:%M:%S%.f"), SpaceSeparated::Naive("%d/%m/%Y %H:%M")],
        (true, true, _, true) => &[SpaceSeparated::Offset("%d/%m/%Y %H:%M:%S%.f%#z"), SpaceSeparated::Offset("%d/%m/%Y %H:%M%#z")],
        (true, false, true, _) => &[SpaceSeparated::Naive("%d/%m/%Y %H.%M.%S")],
        (false | true, false, false, _) => &[],
    }
}

impl TemporalValue {
    /// Attempts to parse a string into a `TemporalValue` using heuristics.
    ///
    /// A byte-shape pre-dispatch inspects the separators the input actually carries and tries only
    /// the one format family that can match, instead of re-scanning the same string through every
    /// supported format in sequence. Formats without timezone info are assumed UTC; a format that
    /// carries a numeric UTC offset has that offset applied.
    #[must_use]
    pub fn try_parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let bytes = s.as_bytes();

        // RFC 3339 is the only supported shape with a 'T'/'t' date-time separator; once one is
        // present, nothing else can match, so dispatch straight to it.
        if bytes.contains(&b'T') || bytes.contains(&b't') {
            return DateTime::parse_from_rfc3339(s).ok().map(|dt| TemporalValue::DateTime(dt.with_timezone(&Utc)));
        }

        if bytes.contains(&b' ') {
            for candidate in space_separated_candidates(s) {
                if let Some(parsed) = candidate.parse(s) {
                    return Some(TemporalValue::DateTime(parsed));
                }
            }
        } else {
            // A bare date: '/' is d/m/Y, '-' is Y-m-d. An all-digit run carries no date separator and
            // cannot form a `NaiveDate`, so it falls through to the timestamp heuristic below.
            let candidates: &[&str] = if bytes.contains(&b'/') {
                &["%d/%m/%Y"]
            } else if bytes.contains(&b'-') {
                &["%Y-%m-%d"]
            } else {
                &[]
            };
            for fmt in candidates {
                if let Ok(d) = NaiveDate::parse_from_str(s, fmt)
                    && let Some(dt) = d.and_hms_opt(0, 0, 0)
                {
                    return Some(TemporalValue::DateTime(Utc.from_utc_datetime(&dt)));
                }
            }
        }

        // A bare number is a Unix epoch. A whole one is read exactly, since a nanosecond epoch
        // exceeds what an `f64` holds; one with a fraction, such as `1775253620.5`, keeps it rather
        // than being truncated to the whole second.
        match s.parse::<i64>() {
            Ok(whole) => Self::from_epoch(whole),
            Err(_) => parse_float(s).and_then(Self::from_fractional_epoch),
        }
    }

    /// Reads a number as a Unix epoch instant in seconds, milliseconds or nanoseconds, the unit
    /// inferred from its magnitude, or returns `None` when it is too small to be an epoch.
    ///
    /// A JSON source writes an epoch as a number as often as as text, and both must denote the same
    /// instant. A fractional number keeps its fraction down to the precision the float carries.
    #[must_use]
    pub fn from_number(number: &Number) -> Option<TemporalValue> {
        match *number {
            Number::Integer(whole) => Self::from_epoch(whole),
            Number::Float(value) => Self::from_fractional_epoch(value),
        }
    }

    /// Reads a whole epoch count, exactly.
    fn from_epoch(whole: i64) -> Option<TemporalValue> {
        let unit = EpochUnit::of(whole)?;
        Self::from_epoch_nanoseconds(i128::from(whole) * i128::from(unit.nanoseconds()))
    }

    /// Reads an epoch count that may carry a fraction, its unit decided by its whole part.
    fn from_fractional_epoch(value: f64) -> Option<TemporalValue> {
        let unit = EpochUnit::of(value.trunc().to_i64()?)?;
        Self::from_epoch_nanoseconds((value * f64::from(unit.nanoseconds())).round().to_i128()?)
    }

    /// Builds the instant `nanoseconds` after the Unix epoch, or `None` when chrono cannot hold it.
    fn from_epoch_nanoseconds(nanoseconds: i128) -> Option<TemporalValue> {
        let per_second = i128::from(EpochUnit::Seconds.nanoseconds());
        let seconds = i64::try_from(nanoseconds.div_euclid(per_second)).ok()?;
        let subsecond = u32::try_from(nanoseconds.rem_euclid(per_second)).ok()?;
        Utc.timestamp_opt(seconds, subsecond).single().map(TemporalValue::DateTime)
    }
}

#[cfg(test)]
mod tests {
    use crate::value::types::{Number, TemporalValue};
    use chrono::{DateTime, Duration, TimeZone, Utc};

    #[test]
    fn an_rfc3339_string_parses_to_a_datetime() {
        assert!(matches!(TemporalValue::try_parse("2023-12-25T10:30:00Z"), Some(TemporalValue::DateTime(_))));
    }

    #[test]
    fn a_date_only_string_parses() {
        assert!(TemporalValue::try_parse("2023-12-25").is_some());
    }

    #[test]
    fn every_supported_format_still_parses_after_byte_shape_dispatch() {
        // One representative input per supported format; the byte-shape dispatch must still route
        // each to the format that parses it.
        let inputs = [
            "2023-12-25T10:30:00Z", // RFC 3339
            "2023-12-25 10:30:00",  // %Y-%m-%d %H:%M:%S
            "2023-12-25 10:30",     // %Y-%m-%d %H:%M
            "25/12/2023 10:30:00",  // %d/%m/%Y %H:%M:%S
            "25/12/2023 10:30",     // %d/%m/%Y %H:%M
            "2023-12-25 10.30.00",  // %Y-%m-%d %H.%M.%S
            "25/12/2023 10.30.00",  // %d/%m/%Y %H.%M.%S
            "2023-12-25",           // %Y-%m-%d
            "25/12/2023",           // %d/%m/%Y
        ];

        for input in inputs {
            assert!(
                matches!(TemporalValue::try_parse(input), Some(TemporalValue::DateTime(_))),
                "expected {input:?} to parse to a datetime"
            );
        }
    }

    /// The instant a spelling denotes, or `None` when it does not parse.
    fn instant(input: &str) -> Option<DateTime<Utc>> {
        TemporalValue::try_parse(input).map(|parsed| match parsed {
            TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => dt,
        })
    }

    /// The instant `2026-03-01T11:04:35Z`, which every zero-offset spelling below denotes.
    fn eleven_oh_four() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 35).unwrap()
    }

    #[test]
    fn a_space_separated_timestamp_reads_the_same_instant_as_its_rfc_3339_spelling() {
        for input in [
            "2026-03-01 11:04:35+00:00",
            "2026-03-01 11:04:35+0000",
            "2026-03-01T11:04:35+00:00",
            "2026-03-01T11:04:35Z",
            "2026-03-01 11:04:35",
            "2026-03-01 11:04:35Z",
        ] {
            assert_eq!(instant(input), Some(eleven_oh_four()), "mismatch for {input:?}");
        }
    }

    #[test]
    fn a_space_separated_offset_is_applied_rather_than_ignored() {
        // 11:04:35 two hours ahead of UTC is 09:04:35 UTC; reading the offset as UTC would keep the
        // hour and silently place the observation two hours late.
        assert_eq!(instant("2026-03-01 11:04:35+02:00"), Some(Utc.with_ymd_and_hms(2026, 3, 1, 9, 4, 35).unwrap()));
        assert_eq!(instant("2026-03-01 11:04:35-05:00"), Some(Utc.with_ymd_and_hms(2026, 3, 1, 16, 4, 35).unwrap()));
        assert_eq!(instant("2026-03-01 11:04:35+0200"), Some(Utc.with_ymd_and_hms(2026, 3, 1, 9, 4, 35).unwrap()));
    }

    #[test]
    fn a_space_separated_timestamp_carries_fractional_seconds() {
        let expected = Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 35).unwrap() + Duration::milliseconds(250);

        assert_eq!(instant("2026-03-01 11:04:35.250"), Some(expected));
        assert_eq!(instant("2026-03-01 11:04:35.250+00:00"), Some(expected));
    }

    #[test]
    fn a_space_separated_timestamp_without_seconds_carries_an_offset() {
        assert_eq!(instant("2026-03-01 11:04+02:00"), Some(Utc.with_ymd_and_hms(2026, 3, 1, 9, 4, 0).unwrap()));
        assert_eq!(instant("2026-03-01 11:04"), Some(Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 0).unwrap()));
    }

    #[test]
    fn a_day_first_space_separated_timestamp_carries_an_offset_too() {
        assert_eq!(instant("01/03/2026 11:04:35+02:00"), Some(Utc.with_ymd_and_hms(2026, 3, 1, 9, 4, 35).unwrap()));
        assert_eq!(instant("01/03/2026 11:04:35"), Some(eleven_oh_four()));
    }

    #[test]
    fn text_that_is_not_a_timestamp_still_parses_to_nothing() {
        for input in ["not a timestamp", "2026-03-01 not a time", "2026-13-01 11:04:35+00:00", "2026-03-01 11:04:35+"] {
            assert_eq!(instant(input), None, "expected {input:?} to parse to nothing");
        }
    }

    #[test]
    fn a_bare_year_is_too_small_for_the_timestamp_heuristic_and_does_not_parse() {
        // A four-digit year carries no date separator and cannot form a `NaiveDate`; the timestamp
        // heuristic then rejects it as too small, so it yields no temporal value.
        assert_eq!(TemporalValue::try_parse("2023"), None);
    }

    #[test]
    fn an_epoch_second_integer_parses_via_the_timestamp_heuristic() {
        assert!(matches!(TemporalValue::try_parse("1700000000"), Some(TemporalValue::DateTime(_))));
    }

    /// The instant a number denotes, or `None` when it is not an epoch.
    fn epoch(number: &Number) -> Option<DateTime<Utc>> {
        TemporalValue::from_number(number).map(|parsed| match parsed {
            TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => dt,
        })
    }

    /// The instant `2026-04-03T22:00:20Z`, 1775253620 seconds after the epoch.
    fn epoch_instant() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 3, 22, 0, 20).unwrap()
    }

    #[test]
    fn an_epoch_number_reads_the_same_instant_in_seconds_milliseconds_and_nanoseconds() {
        assert_eq!(epoch(&Number::Integer(1_775_253_620)), Some(epoch_instant()));
        assert_eq!(epoch(&Number::Integer(1_775_253_620_000)), Some(epoch_instant()));
        assert_eq!(epoch(&Number::Integer(1_775_253_620_000_000_000)), Some(epoch_instant()));
    }

    #[test]
    fn an_epoch_number_reads_the_same_instant_as_its_text() {
        for (number, text) in [
            (1_775_253_620, "1775253620"),
            (1_775_253_620_250, "1775253620250"),
            (-1_775_253_620, "-1775253620"),
        ] {
            assert_eq!(epoch(&Number::Integer(number)), instant(text), "mismatch for {number}");
        }
    }

    #[test]
    fn a_whole_float_epoch_reads_like_the_integer() {
        assert_eq!(epoch(&Number::Float(1_775_253_620.0)), Some(epoch_instant()));
    }

    #[test]
    fn a_fractional_epoch_keeps_its_fraction_as_a_number_and_as_text() {
        let expected = epoch_instant() + Duration::milliseconds(500);

        assert_eq!(epoch(&Number::Float(1_775_253_620.5)), Some(expected));
        assert_eq!(epoch(&Number::Float(1_775_253_620_500.0)), Some(expected));
        assert_eq!(instant("1775253620.5"), Some(expected));
    }

    #[test]
    fn a_number_too_small_for_an_epoch_is_not_an_instant() {
        assert_eq!(epoch(&Number::Integer(2026)), None);
        assert_eq!(epoch(&Number::Float(2026.5)), None);
        assert_eq!(instant("2026.5"), None);
    }

    #[test]
    fn a_number_that_is_not_finite_is_not_an_instant() {
        assert_eq!(epoch(&Number::Float(f64::NAN)), None);
        assert_eq!(epoch(&Number::Float(f64::INFINITY)), None);
    }
}

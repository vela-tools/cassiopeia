use crate::netcdf::error::NetCdfIngestError;
use calendrical_calculations::{
    gregorian::{self, gregorian_from_fixed},
    julian::fixed_from_julian,
};
use netcdf_reader::cf::{CfCalendar, CfDate, CfDateTime, CfTimeOfDay, CfTimeRef};
use strum::{Display, EnumString};

/// The first day of the `standard` calendar's Gregorian part, which follows Julian 1582-10-04 (CF 1.11
/// §4.4.2).
const FIRST_GREGORIAN_DAY: (i32, u8, u8) = (1582, 10, 15);

/// A CF calendar (CF 1.11 §4.4.2), read from a `calendar` attribute whatever its ASCII case and written
/// by its CF name.
///
/// `utc`, `tai` and `none` are not among them, because the decoder performs neither their leap-second
/// arithmetic nor calendar-free counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum Calendar {
    /// The mixed calendar: Julian before 1582-10-15, Gregorian from then on. Also named `gregorian`.
    #[strum(to_string = "standard", serialize = "gregorian")]
    Standard,
    /// The Gregorian calendar, extended back before 1582-10-15.
    #[strum(to_string = "proleptic_gregorian")]
    ProlepticGregorian,
    /// The Julian calendar, with a leap year every fourth year.
    #[strum(to_string = "julian")]
    Julian,
    /// A model calendar of 365-day years. Also named `365_day`.
    #[strum(to_string = "noleap", serialize = "365_day")]
    NoLeap,
    /// A model calendar of 366-day years. Also named `366_day`.
    #[strum(to_string = "all_leap", serialize = "366_day")]
    AllLeap,
    /// A model calendar of twelve 30-day months.
    #[strum(to_string = "360_day")]
    Day360,
}

impl Calendar {
    /// The calendar the decoder counts this calendar's days in.
    #[must_use]
    pub const fn decoder(self) -> CfCalendar {
        match self {
            Calendar::Standard => CfCalendar::Standard,
            Calendar::ProlepticGregorian => CfCalendar::ProlepticGregorian,
            Calendar::Julian => CfCalendar::Julian,
            Calendar::NoLeap => CfCalendar::NoLeap,
            Calendar::AllLeap => CfCalendar::AllLeap,
            Calendar::Day360 => CfCalendar::Day360,
        }
    }

    /// `reference`, read in this calendar, restated so that a value decoded against it is written as the
    /// date RFC 3339 means.
    ///
    /// RFC 3339 and `xsd:dateTime` state dates in the proleptic Gregorian calendar. A time value is the
    /// real time elapsed since its epoch (CF 1.11 §4.4.1), so in a calendar of real days, `standard`,
    /// `proleptic_gregorian` or `julian`, it names exactly one instant. The epoch is restated as the
    /// Gregorian date of its own day, and values are counted from there with Gregorian arithmetic. That
    /// moves a `julian` epoch, whose dates run 13 days behind the Gregorian ones from Julian 1900-03-01
    /// to 2100-02-28, and a `standard` epoch before 1582-10-15, which is a Julian date. A model
    /// calendar's dates name no real day, so its reference is kept and its dates keep their labels.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError::EpochOutOfRange`] when the epoch's Gregorian year does not fit in 32
    /// bits.
    pub fn gregorian_reference(self, reference: CfTimeRef) -> Result<CfTimeRef, NetCdfIngestError> {
        let CfDateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanosecond,
            ..
        } = reference.epoch;
        let (year, month, day) = match self {
            Calendar::Julian => julian_to_gregorian(year, month, day)?,
            Calendar::Standard if (year, month, day) < FIRST_GREGORIAN_DAY => julian_to_gregorian(year, month, day)?,
            Calendar::Standard | Calendar::ProlepticGregorian => (year, month, day),
            Calendar::NoLeap | Calendar::AllLeap | Calendar::Day360 => return Ok(reference),
        };
        Ok(CfTimeRef {
            unit: reference.unit,
            epoch: CfDateTime::new(
                CfCalendar::ProlepticGregorian,
                CfDate::new(year, month, day),
                CfTimeOfDay::new(hour, minute, second, nanosecond)?,
            )?,
            calendar: CfCalendar::ProlepticGregorian,
        })
    }
}

/// Whether the proleptic Gregorian calendar has the date `year`-`month`-`day`.
///
/// A model calendar's date may be missing from it: February 30 of `360_day`, and February 29 of a
/// Gregorian common year in `360_day` and `all_leap`.
#[must_use]
pub const fn gregorian_has(year: i32, month: u8, day: u8) -> bool {
    let length = match month {
        2 if gregorian::is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _other => 31,
    };
    day <= length
}

/// The proleptic Gregorian date of the day the Julian calendar names `year`-`month`-`day`.
fn julian_to_gregorian(year: i32, month: u8, day: u8) -> Result<(i32, u8, u8), NetCdfIngestError> {
    Ok(gregorian_from_fixed(fixed_from_julian(year, month, day))?)
}

#[cfg(test)]
mod tests {
    use crate::netcdf::{
        calendar::{Calendar, gregorian_has},
        error::NetCdfIngestError,
    };
    use netcdf_reader::cf::{CfCalendar, CfTimeRef, parse_time_units};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    /// The reference `units` state in `calendar`, restated for RFC 3339.
    fn restated(units: &str, calendar: Calendar) -> Result<CfTimeRef, NetCdfIngestError> {
        calendar.gregorian_reference(parse_time_units(units, calendar.decoder()).unwrap())
    }

    /// The epoch of `reference` as `year`-`month`-`day` and the calendar it is counted in.
    fn epoch(reference: &CfTimeRef) -> ((i32, u8, u8), CfCalendar) {
        ((reference.epoch.year, reference.epoch.month, reference.epoch.day), reference.calendar)
    }

    #[test]
    fn every_cf_calendar_name_and_alias_is_read_whatever_its_case() {
        let read = [
            ("standard", Calendar::Standard),
            ("GREGORIAN", Calendar::Standard),
            ("proleptic_gregorian", Calendar::ProlepticGregorian),
            ("Julian", Calendar::Julian),
            ("noleap", Calendar::NoLeap),
            ("365_day", Calendar::NoLeap),
            ("all_leap", Calendar::AllLeap),
            ("366_day", Calendar::AllLeap),
            ("360_day", Calendar::Day360),
        ];
        for (name, calendar) in read {
            assert_eq!(Calendar::from_str(name), Ok(calendar), "{name}");
        }
    }

    #[test]
    fn utc_tai_none_and_unknown_names_are_not_calendars() {
        for name in ["utc", "tai", "none", "lunar", ""] {
            assert!(Calendar::from_str(name).is_err(), "{name}");
        }
    }

    #[test]
    fn a_calendar_is_written_by_its_cf_name_rather_than_its_alias() {
        assert_eq!(Calendar::Standard.to_string(), "standard");
        assert_eq!(Calendar::ProlepticGregorian.to_string(), "proleptic_gregorian");
        assert_eq!(Calendar::Julian.to_string(), "julian");
        assert_eq!(Calendar::NoLeap.to_string(), "noleap");
        assert_eq!(Calendar::AllLeap.to_string(), "all_leap");
        assert_eq!(Calendar::Day360.to_string(), "360_day");
    }

    #[test]
    fn each_calendar_is_decoded_in_the_decoder_calendar_of_the_same_name() {
        assert_eq!(Calendar::Standard.decoder(), CfCalendar::Standard);
        assert_eq!(Calendar::ProlepticGregorian.decoder(), CfCalendar::ProlepticGregorian);
        assert_eq!(Calendar::Julian.decoder(), CfCalendar::Julian);
        assert_eq!(Calendar::NoLeap.decoder(), CfCalendar::NoLeap);
        assert_eq!(Calendar::AllLeap.decoder(), CfCalendar::AllLeap);
        assert_eq!(Calendar::Day360.decoder(), CfCalendar::Day360);
    }

    #[test]
    fn a_julian_epoch_is_restated_as_the_gregorian_date_of_its_day_keeping_its_time() {
        let reference = restated("hours since 2026-01-01 06:30:00", Calendar::Julian).unwrap();
        assert_eq!(epoch(&reference), ((2026, 1, 14), CfCalendar::ProlepticGregorian));
        assert_eq!((reference.epoch.hour, reference.epoch.minute), (6, 30));
    }

    #[test]
    fn a_julian_only_february_29_epoch_is_restated_as_the_gregorian_date_of_its_day() {
        assert_eq!(
            epoch(&restated("days since 1900-02-29", Calendar::Julian).unwrap()),
            ((1900, 3, 13), CfCalendar::ProlepticGregorian)
        );
    }

    #[test]
    fn a_standard_epoch_before_the_reform_is_restated_from_julian_and_one_after_it_is_kept() {
        assert_eq!(
            epoch(&restated("days since 1582-10-04", Calendar::Standard).unwrap()),
            ((1582, 10, 14), CfCalendar::ProlepticGregorian)
        );
        assert_eq!(
            epoch(&restated("days since 1500-01-01", Calendar::Standard).unwrap()),
            ((1500, 1, 10), CfCalendar::ProlepticGregorian)
        );
        assert_eq!(
            epoch(&restated("days since 1582-10-15", Calendar::Standard).unwrap()),
            ((1582, 10, 15), CfCalendar::ProlepticGregorian)
        );
    }

    #[test]
    fn a_proleptic_gregorian_epoch_is_kept() {
        assert_eq!(
            epoch(&restated("days since 1500-01-01", Calendar::ProlepticGregorian).unwrap()),
            ((1500, 1, 1), CfCalendar::ProlepticGregorian)
        );
    }

    #[test]
    fn a_model_calendar_keeps_its_reference() {
        assert_eq!(
            epoch(&restated("days since 2026-02-30", Calendar::Day360).unwrap()),
            ((2026, 2, 30), CfCalendar::Day360)
        );
        assert_eq!(
            epoch(&restated("days since 2026-02-29", Calendar::AllLeap).unwrap()),
            ((2026, 2, 29), CfCalendar::AllLeap)
        );
        assert_eq!(
            epoch(&restated("days since 2026-03-01", Calendar::NoLeap).unwrap()),
            ((2026, 3, 1), CfCalendar::NoLeap)
        );
    }

    #[test]
    fn a_julian_epoch_whose_gregorian_year_does_not_fit_in_32_bits_is_refused() {
        let error = restated("days since 2147483647-12-31", Calendar::Julian).unwrap_err();
        assert!(matches!(error, NetCdfIngestError::EpochOutOfRange(_)), "{error:?}");
    }

    #[test]
    fn the_gregorian_calendar_has_february_29_only_in_its_leap_years_and_never_february_30() {
        assert!(gregorian_has(2024, 2, 29));
        assert!(gregorian_has(2000, 2, 29));
        assert!(!gregorian_has(2026, 2, 29));
        assert!(!gregorian_has(1900, 2, 29));
        assert!(!gregorian_has(2024, 2, 30));
        assert!(gregorian_has(2026, 2, 28));
    }

    #[test]
    fn the_gregorian_calendar_has_the_last_day_of_every_month_of_thirty_or_thirty_one_days() {
        assert!(gregorian_has(2026, 1, 31));
        assert!(gregorian_has(2026, 4, 30));
        assert!(!gregorian_has(2026, 4, 31));
        assert!(gregorian_has(2026, 12, 31));
    }
}

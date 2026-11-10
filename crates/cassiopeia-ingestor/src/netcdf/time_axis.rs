use crate::netcdf::{
    calendar::{Calendar, gregorian_has},
    cf_attribute::CfAttribute,
    decoded_time::DecodedTime,
    epoch_zone::EpochZone,
    error::NetCdfIngestError,
    time_unit::TimeUnitScale,
};
use netcdf_reader::{
    NcVariable,
    cf::{CfTimeRef, CfTimeUnit, decode_time_exact, parse_time_units},
};

/// The microseconds in one second.
const MICROSECONDS_PER_SECOND: f64 = 1e6;

/// The seconds in one day.
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Whether `units` take the CF `<unit> since <epoch>` form of a time coordinate (CF 1.11 §4.4).
///
/// The comparison folds ASCII case only, so it never shifts the byte offsets of non-ASCII units.
#[must_use]
pub fn declares_epoch(units: &str) -> bool {
    units.to_ascii_lowercase().contains(" since ")
}

/// A CF time coordinate's reference: its unit, epoch and calendar, ready to decode values into
/// RFC 3339 timestamps.
#[derive(Debug, Clone)]
pub struct TimeAxis {
    /// The reference values are decoded against: in a calendar of real days, the file's own restated
    /// in the proleptic Gregorian calendar.
    reference: CfTimeRef,
    scale: TimeUnitScale,
    /// The calendar the file states its dates in.
    calendar: Calendar,
}

impl TimeAxis {
    /// Reads the time reference of `variable` from its `units` and `calendar` attributes.
    ///
    /// Returns `None` when the variable's units are not `<unit> since <epoch>`. Three guards run before
    /// the CF decoder sees the units: non-ASCII units are refused, because the decoder's unit splitter
    /// assumes lowercasing keeps byte offsets; a `calendar` outside the CF set is refused, because the
    /// decoder would read it as the standard calendar; and a UTC zone designator, which UDUNITS allows
    /// but the decoder rejects, is removed, while any other offset is refused. A UDUNITS time unit the
    /// decoder does not spell, such as `sec` or `milliseconds`, is then rewritten to one it does. The
    /// reference of a calendar of real days is restated in the proleptic Gregorian calendar, as
    /// [`Calendar::gregorian_reference`] describes.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError::NonAsciiTimeUnits`], [`NetCdfIngestError::UnknownCalendar`] or
    /// [`NetCdfIngestError::UnsupportedTimeZone`] for the guarded cases,
    /// [`NetCdfIngestError::Read`] when the decoder cannot parse the units, and
    /// [`NetCdfIngestError::EpochOutOfRange`] when the restated epoch's year does not fit in 32 bits.
    pub fn from_variable(variable: &NcVariable) -> Result<Option<TimeAxis>, NetCdfIngestError> {
        let Some(units) = CfAttribute::Units.text(variable.attributes()).filter(|units| declares_epoch(units)) else {
            return Ok(None);
        };
        if !units.is_ascii() {
            return Err(NetCdfIngestError::NonAsciiTimeUnits {
                variable: variable.name().to_owned(),
            });
        }
        let calendar = match CfAttribute::Calendar.text(variable.attributes()) {
            Some(name) => name.parse::<Calendar>().map_err(|_| NetCdfIngestError::UnknownCalendar {
                variable: variable.name().to_owned(),
                calendar: name.to_owned(),
            })?,
            None => Calendar::Standard,
        };
        let units = match EpochZone::split(units) {
            EpochZone::Utc(units) => units,
            EpochZone::Offset => {
                return Err(NetCdfIngestError::UnsupportedTimeZone {
                    variable: variable.name().to_owned(),
                    units: units.to_owned(),
                });
            }
        };
        let (units, scale) = TimeUnitScale::rewrite(units);
        Ok(Some(TimeAxis {
            reference: calendar.gregorian_reference(parse_time_units(&units, calendar.decoder())?)?,
            scale,
            calendar,
        }))
    }

    /// The calendar the file states the coordinate's dates in.
    #[must_use]
    pub const fn calendar(&self) -> Calendar {
        self.calendar
    }

    /// Decodes a coordinate value into an RFC 3339 timestamp, `YYYY-MM-DDTHH:MM:SS[.fraction]+00:00`.
    ///
    /// The `+00:00` offset matches the timestamps GRIB records carry. The time is rounded to the
    /// microsecond, the fraction written with its trailing zeros trimmed and left out for a whole
    /// second. A double counting days since 1950 resolves no finer than about a third of a
    /// microsecond, and the decoder's single multiplication into nanoseconds adds a quarter of a
    /// microsecond more, so an unrounded `OceanSITES` time of 04:10 reads 04:09:59.999999744. The value is
    /// therefore split into whole days, which the decoder places exactly for some eighteen thousand
    /// years either side of the epoch, and the seconds of the rest of the day, rounded to the
    /// microsecond and added through the decoder's calendar arithmetic so a rounding up to the next
    /// minute or day carries correctly.
    ///
    /// A value in a calendar of real days is written as the proleptic Gregorian date of its instant,
    /// so a `julian` date reads 13 days later than the file labels it in the 20th and 21st centuries. A
    /// model calendar's date keeps its label, and one the Gregorian calendar does not have, such as
    /// February 30 in `360_day`, is returned as [`DecodedTime::Nonexistent`].
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError::Read`] when the value is not finite or its date falls outside the
    /// calendar's range.
    pub fn decode(&self, value: f64) -> Result<DecodedTime, NetCdfIngestError> {
        let value = self.scale.to_decoder_unit(value);
        let time = match seconds_per_unit(self.reference.unit) {
            Some(seconds) => {
                let units_per_day = SECONDS_PER_DAY / seconds;
                let days = (value / units_per_day).floor();
                let rest = ((value - days * units_per_day) * seconds * MICROSECONDS_PER_SECOND).round() / MICROSECONDS_PER_SECOND;
                let in_days = CfTimeRef {
                    unit: CfTimeUnit::Days,
                    epoch: self.reference.epoch,
                    calendar: self.reference.calendar,
                };
                let day = CfTimeRef {
                    unit: CfTimeUnit::Seconds,
                    epoch: decode_time_exact(days, &in_days)?,
                    calendar: self.reference.calendar,
                };
                decode_time_exact(rest, &day)?
            }
            None => decode_time_exact(value, &self.reference)?,
        };
        let nanoseconds = format!("{:09}", time.nanosecond);
        let fraction = nanoseconds.trim_end_matches('0');
        let separator = if fraction.is_empty() { "" } else { "." };
        let timestamp = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{separator}{fraction}+00:00",
            time.year, time.month, time.day, time.hour, time.minute, time.second
        );
        Ok(if gregorian_has(time.year, time.month, time.day) {
            DecodedTime::Gregorian(timestamp)
        } else {
            DecodedTime::Nonexistent(timestamp)
        })
    }
}

/// The seconds in one `unit`, or `None` for months, which the decoder counts only in whole 360-day
/// calendar months and so never splits.
const fn seconds_per_unit(unit: CfTimeUnit) -> Option<f64> {
    match unit {
        CfTimeUnit::Seconds => Some(1.0),
        CfTimeUnit::Minutes => Some(60.0),
        CfTimeUnit::Hours => Some(3_600.0),
        CfTimeUnit::Days => Some(SECONDS_PER_DAY),
        CfTimeUnit::Months => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::{calendar::Calendar, decoded_time::DecodedTime, error::NetCdfIngestError, time_axis::TimeAxis};
    use netcdf_reader::{NcAttrValue, NcAttribute, NcType, NcVariable};
    use pretty_assertions::assert_eq;

    fn variable(attributes: &[(&str, &str)]) -> NcVariable {
        NcVariable {
            name: "time".to_string(),
            dimensions: Vec::new(),
            dtype: NcType::Double,
            attributes: attributes
                .iter()
                .map(|(name, value)| NcAttribute {
                    name: (*name).to_string(),
                    value: NcAttrValue::Chars((*value).to_string()),
                })
                .collect(),
            data_offset: 0,
            _data_size: 0,
            is_record_var: false,
            record_size: 0,
        }
    }

    fn decode(attributes: &[(&str, &str)], value: f64) -> Result<DecodedTime, NetCdfIngestError> {
        TimeAxis::from_variable(&variable(attributes))?.unwrap().decode(value)
    }

    /// `value` days since `epoch` in `calendar`, decoded.
    fn day(epoch: &str, calendar: &str, value: f64) -> DecodedTime {
        decode(&[("units", &format!("days since {epoch}")), ("calendar", calendar)], value).unwrap()
    }

    fn gregorian(timestamp: &str) -> DecodedTime {
        DecodedTime::Gregorian(timestamp.to_owned())
    }

    fn nonexistent(timestamp: &str) -> DecodedTime {
        DecodedTime::Nonexistent(timestamp.to_owned())
    }

    #[test]
    fn fractional_days_decode_to_an_rfc3339_timestamp() {
        assert_eq!(
            decode(&[("units", "days since 2026-01-01")], 1.5).unwrap(),
            gregorian("2026-01-02T12:00:00+00:00")
        );
    }

    #[test]
    fn a_z_designator_on_the_epoch_is_read_as_utc() {
        assert_eq!(
            decode(&[("units", "seconds since 1970-01-01T00:00:00Z")], 0.0).unwrap(),
            gregorian("1970-01-01T00:00:00+00:00")
        );
    }

    #[test]
    fn a_utc_designator_on_the_epoch_is_read_as_utc() {
        assert_eq!(
            decode(&[("units", "hours since 2026-01-01 00:00:00 UTC")], 6.0).unwrap(),
            gregorian("2026-01-01T06:00:00+00:00")
        );
    }

    #[test]
    fn the_unsigned_zero_offset_of_the_ncei_templates_is_read_as_utc() {
        assert_eq!(
            decode(&[("units", "seconds since 1970-01-01 00:00:00 0:00")], 10.0).unwrap(),
            gregorian("1970-01-01T00:00:10+00:00")
        );
    }

    #[test]
    fn a_non_zero_epoch_offset_is_refused() {
        let error = TimeAxis::from_variable(&variable(&[("units", "hours since 2026-01-01T00:00:00+02:00")])).unwrap_err();
        assert!(matches!(error, NetCdfIngestError::UnsupportedTimeZone { .. }));
    }

    #[test]
    fn a_fraction_of_a_second_keeps_only_its_significant_digits() {
        assert_eq!(
            decode(&[("units", "seconds since 2026-01-01")], 0.25).unwrap(),
            gregorian("2026-01-01T00:00:00.25+00:00")
        );
    }

    #[test]
    fn a_julian_date_is_written_as_the_gregorian_date_of_its_day() {
        // Julian dates run 13 days behind Gregorian ones from Julian 1900-03-01 to 2100-02-28.
        assert_eq!(day("2026-01-01", "julian", 0.0), gregorian("2026-01-14T00:00:00+00:00"));
        assert_eq!(day("2026-01-01", "julian", 365.0), gregorian("2027-01-14T00:00:00+00:00"));
    }

    #[test]
    fn a_february_29_only_the_julian_calendar_has_is_written_as_the_gregorian_date_of_its_day() {
        assert_eq!(day("1900-02-28", "julian", 1.0), gregorian("1900-03-13T00:00:00+00:00"));
    }

    #[test]
    fn a_julian_time_of_day_and_fraction_survive_the_restatement() {
        assert_eq!(
            decode(&[("units", "hours since 2026-01-01 06:00:00"), ("calendar", "julian")], 1.5).unwrap(),
            gregorian("2026-01-14T07:30:00+00:00")
        );
    }

    #[test]
    fn a_standard_date_before_the_reform_is_written_in_the_proleptic_gregorian_calendar() {
        assert_eq!(day("1582-10-15", "standard", -1.0), gregorian("1582-10-14T00:00:00+00:00"));
        assert_eq!(day("1500-01-01", "gregorian", 0.0), gregorian("1500-01-10T00:00:00+00:00"));
    }

    #[test]
    fn a_standard_count_from_a_julian_epoch_runs_across_the_reform_in_real_days() {
        // In the standard calendar Julian 1582-10-04 is followed by Gregorian 1582-10-15.
        assert_eq!(day("1582-10-04", "standard", 1.0), gregorian("1582-10-15T00:00:00+00:00"));
    }

    #[test]
    fn a_proleptic_gregorian_date_before_the_reform_is_written_as_the_file_states_it() {
        assert_eq!(day("1582-10-15", "proleptic_gregorian", -1.0), gregorian("1582-10-14T00:00:00+00:00"));
    }

    #[test]
    fn a_360_day_date_keeps_its_label_and_one_the_gregorian_calendar_lacks_is_nonexistent() {
        assert_eq!(day("2026-01-01", "360_day", 30.0), gregorian("2026-02-01T00:00:00+00:00"));
        // 2026 is a Gregorian common year, so it has neither February 29 nor February 30.
        assert_eq!(day("2026-01-01", "360_day", 58.0), nonexistent("2026-02-29T00:00:00+00:00"));
        assert_eq!(day("2026-01-01", "360_day", 59.0), nonexistent("2026-02-30T00:00:00+00:00"));
        assert_eq!(day("2024-01-01", "360_day", 58.0), gregorian("2024-02-29T00:00:00+00:00"));
    }

    #[test]
    fn an_all_leap_february_29_is_nonexistent_only_in_a_gregorian_common_year() {
        assert_eq!(day("2026-01-01", "all_leap", 59.0), nonexistent("2026-02-29T00:00:00+00:00"));
        assert_eq!(day("2024-01-01", "366_day", 59.0), gregorian("2024-02-29T00:00:00+00:00"));
    }

    #[test]
    fn a_noleap_date_keeps_its_label_even_where_the_gregorian_year_is_a_leap_year() {
        assert_eq!(day("2024-01-01", "noleap", 59.0), gregorian("2024-03-01T00:00:00+00:00"));
    }

    #[test]
    fn whole_360_day_months_keep_their_labels() {
        assert_eq!(
            decode(&[("units", "months since 2026-01-30"), ("calendar", "360_day")], 1.0).unwrap(),
            nonexistent("2026-02-30T00:00:00+00:00")
        );
    }

    #[test]
    fn the_axis_names_the_calendar_the_file_states_its_dates_in() {
        let axis = |calendar: &str| {
            TimeAxis::from_variable(&variable(&[("units", "days since 2026-01-01"), ("calendar", calendar)]))
                .unwrap()
                .unwrap()
                .calendar()
        };
        assert_eq!(axis("julian"), Calendar::Julian);
        assert_eq!(axis("360_DAY"), Calendar::Day360);
        assert_eq!(
            TimeAxis::from_variable(&variable(&[("units", "days since 2026-01-01")]))
                .unwrap()
                .unwrap()
                .calendar(),
            Calendar::Standard
        );
    }

    #[test]
    fn a_julian_epoch_whose_gregorian_year_does_not_fit_in_32_bits_is_refused() {
        let error = TimeAxis::from_variable(&variable(&[("units", "days since 2147483647-12-31"), ("calendar", "julian")])).unwrap_err();
        assert!(matches!(error, NetCdfIngestError::EpochOutOfRange(_)), "{error:?}");
    }

    #[test]
    fn an_unknown_calendar_is_refused() {
        let error = TimeAxis::from_variable(&variable(&[("units", "days since 2026-01-01"), ("calendar", "lunar")])).unwrap_err();
        assert!(matches!(error, NetCdfIngestError::UnknownCalendar { .. }));
    }

    #[test]
    fn non_ascii_units_are_refused_without_panicking() {
        let error = TimeAxis::from_variable(&variable(&[("units", "\u{212A}\u{212A} since 2000-01-01")])).unwrap_err();
        assert!(matches!(error, NetCdfIngestError::NonAsciiTimeUnits { .. }));
    }

    #[test]
    fn without_a_calendar_attribute_the_standard_calendar_applies() {
        // 2024 is a Gregorian leap year, so day 59 is February 29 in the standard calendar.
        assert_eq!(
            decode(&[("units", "days since 2024-01-01")], 59.0).unwrap(),
            gregorian("2024-02-29T00:00:00+00:00")
        );
    }

    #[test]
    fn fractional_days_of_a_ten_minute_series_decode_to_the_whole_minute() {
        // OceanSITES T0N140W: 23634.17361111111 days since 1950 is 2014-09-16 04:10, which a single
        // multiplication into nanoseconds puts at 04:09:59.999999744.
        let value = 23_634.0 + 250.0 / 1_440.0;
        assert_eq!(
            decode(&[("units", "days since 1950-01-01T00:00:00Z")], value).unwrap(),
            gregorian("2014-09-16T04:10:00+00:00")
        );
    }

    #[test]
    fn a_fraction_rounding_up_to_the_next_day_carries_into_the_date() {
        assert_eq!(
            decode(&[("units", "days since 2026-02-28")], 0.999_999_999_999).unwrap(),
            gregorian("2026-03-01T00:00:00+00:00")
        );
    }

    #[test]
    fn whole_seconds_far_from_their_epoch_decode_without_noise() {
        // 7,076,678,401 seconds after 1800-01-01 is 2024-04-02 00:00:01; in nanoseconds that is past
        // 2^62, where a double is spaced 1,024 ns apart.
        assert_eq!(
            decode(&[("units", "seconds since 1800-01-01")], 7_076_678_401.0).unwrap(),
            gregorian("2024-04-02T00:00:01+00:00")
        );
    }

    #[test]
    fn a_negative_fraction_counts_back_from_the_epoch() {
        assert_eq!(
            decode(&[("units", "hours since 2026-01-01")], -1.5).unwrap(),
            gregorian("2025-12-31T22:30:00+00:00")
        );
    }

    #[test]
    fn the_cf_sec_abbreviation_decodes_as_seconds() {
        assert_eq!(
            decode(&[("units", "sec since 1970-01-01")], 90.0).unwrap(),
            gregorian("1970-01-01T00:01:30+00:00")
        );
    }

    #[test]
    fn milliseconds_since_the_epoch_keep_their_exact_fraction() {
        assert_eq!(
            decode(&[("units", "milliseconds since 1970-01-01T00:00:00Z")], 1_717_200_000_123.0).unwrap(),
            gregorian("2024-06-01T00:00:00.123+00:00")
        );
    }

    #[test]
    fn units_without_an_epoch_are_not_a_time_axis() {
        assert!(TimeAxis::from_variable(&variable(&[("units", "K")])).unwrap().is_none());
        assert!(TimeAxis::from_variable(&variable(&[])).unwrap().is_none());
    }
}

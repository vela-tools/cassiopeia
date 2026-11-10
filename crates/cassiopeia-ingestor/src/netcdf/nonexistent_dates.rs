use crate::netcdf::{calendar::Calendar, decoded_time::DecodedTime, variable_name::VariableName};
use netcdf_reader::NcVariable;
use std::num::NonZeroU64;

/// The values of one time variable whose dates the Gregorian calendar does not have.
///
/// Only a model calendar has such dates: February 30 of `360_day`, and February 29 of a Gregorian
/// common year in `360_day` and `all_leap`. They name no instant, so no conversion places them, and the
/// records keep each as it is labelled, which no RFC 3339 reader accepts.
#[derive(Debug, PartialEq, Eq)]
pub struct NonexistentDates {
    /// The time variable.
    pub variable: VariableName,
    /// The calendar its dates are stated in.
    pub calendar: Calendar,
    /// The first of its values that is such a date, as the records carry it.
    pub first: String,
    /// How many of its values are such dates.
    pub count: NonZeroU64,
}

impl NonexistentDates {
    /// The dates among `times`, the decoded values of `variable` in `calendar`, that the Gregorian
    /// calendar does not have, or `None` when it has every one.
    #[must_use]
    pub fn among(variable: &NcVariable, calendar: Calendar, times: &[Option<DecodedTime>]) -> Option<NonexistentDates> {
        let mut nonexistent = times.iter().flatten().filter_map(DecodedTime::nonexistent);
        // The records keep their own copy of the date; this one names it in the report.
        let first = nonexistent.next()?.to_owned();
        let rest = u64::try_from(nonexistent.count()).unwrap_or(u64::MAX);
        Some(NonexistentDates {
            variable: VariableName::from(variable.name()),
            calendar,
            first,
            count: NonZeroU64::MIN.saturating_add(rest),
        })
    }
}

/// The nonexistent dates of each time variable a file's records carry, gathered while the file is read.
#[derive(Debug, Default)]
pub struct NonexistentDateLog {
    variables: Vec<NonexistentDates>,
}

impl NonexistentDateLog {
    /// Keeps `dates`, unless the dates of their variable were already kept.
    ///
    /// A time coordinate shared by several groups of grid variables is read once for each group, with
    /// the same values each time, so only its first reading is kept.
    pub fn record(&mut self, dates: NonexistentDates) {
        if self.variables.iter().all(|kept| kept.variable != dates.variable) {
            self.variables.push(dates);
        }
    }

    /// The time variables holding dates the Gregorian calendar does not have, in the order they were
    /// first read.
    #[must_use]
    pub fn variables(&self) -> &[NonexistentDates] {
        &self.variables
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::{
        calendar::Calendar,
        decoded_time::DecodedTime,
        nonexistent_dates::{NonexistentDateLog, NonexistentDates},
        variable_name::VariableName,
    };
    use netcdf_reader::{NcType, NcVariable};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU64;

    fn variable(name: &str) -> NcVariable {
        NcVariable {
            name: name.to_string(),
            dimensions: Vec::new(),
            dtype: NcType::Double,
            attributes: Vec::new(),
            data_offset: 0,
            _data_size: 0,
            is_record_var: false,
            record_size: 0,
        }
    }

    fn gregorian(timestamp: &str) -> DecodedTime {
        DecodedTime::Gregorian(timestamp.to_owned())
    }

    fn nonexistent(timestamp: &str) -> DecodedTime {
        DecodedTime::Nonexistent(timestamp.to_owned())
    }

    fn dates(name: &str, first: &str, count: u64) -> NonexistentDates {
        NonexistentDates {
            variable: VariableName::from(name),
            calendar: Calendar::Day360,
            first: first.to_owned(),
            count: NonZeroU64::new(count).unwrap(),
        }
    }

    #[test]
    fn times_the_gregorian_calendar_all_has_hold_no_nonexistent_dates() {
        let times = [Some(gregorian("2026-01-30T00:00:00+00:00")), None, Some(gregorian("2026-02-01T00:00:00+00:00"))];
        assert_eq!(NonexistentDates::among(&variable("time"), Calendar::Day360, &times), None);
    }

    #[test]
    fn nonexistent_dates_name_the_first_and_count_every_one_skipping_missing_values() {
        let times = [
            Some(gregorian("2026-01-30T00:00:00+00:00")),
            None,
            Some(nonexistent("2026-02-29T00:00:00+00:00")),
            Some(nonexistent("2026-02-30T00:00:00+00:00")),
        ];
        assert_eq!(
            NonexistentDates::among(&variable("time"), Calendar::Day360, &times),
            Some(dates("time", "2026-02-29T00:00:00+00:00", 2))
        );
    }

    #[test]
    fn a_variable_read_again_is_kept_once_and_another_variable_after_it() {
        let mut log = NonexistentDateLog::default();
        log.record(dates("time", "2026-02-29T00:00:00+00:00", 2));
        log.record(dates("time", "2026-02-29T00:00:00+00:00", 2));
        log.record(dates("valid_time", "2026-02-30T00:00:00+00:00", 1));

        assert_eq!(
            log.variables(),
            [
                dates("time", "2026-02-29T00:00:00+00:00", 2),
                dates("valid_time", "2026-02-30T00:00:00+00:00", 1)
            ]
        );
    }
}

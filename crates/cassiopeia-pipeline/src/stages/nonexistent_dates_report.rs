use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ingestor::netcdf::nonexistent_dates::NonexistentDates;
use cassiopeia_reporter::reporter::DiagnosticSink;

/// The one-line summary: the variable, the file, the calendar, how many dates, and the first.
///
/// The first date rides on the headline because the concise form shows nothing else, and it is what
/// lets a reader find the values in the file.
fn headline(source: &Input, dates: &NonexistentDates) -> String {
    let NonexistentDates {
        variable,
        calendar,
        first,
        count,
    } = dates;
    let held = match count.get() {
        1 => format!("a date of the `{calendar}` calendar that the Gregorian calendar does not have, `{first}`, so the records keep it"),
        count => format!("{count} dates of the `{calendar}` calendar that the Gregorian calendar does not have, the first `{first}`, so the records keep them"),
    };
    format!("The time variable `{variable}` of `{source}` holds {held} as labelled")
}

/// Why the dates are kept as labelled, which the run summary shows for the code: a model calendar's
/// day the Gregorian calendar lacks names no instant.
///
/// The variable is named so two variables of one file stay two findings.
fn cause(dates: &NonexistentDates) -> String {
    format!(
        "a day of the `{}` calendar of `{}` that the Gregorian calendar does not have names no instant, so a `datetime` transformation cannot read its date",
        dates.calendar, dates.variable
    )
}

/// Warns once for each time variable of a `NetCDF` file holding dates the Gregorian calendar does not
/// have, and returns how many such dates there were.
///
/// One diagnostic per variable, carrying each of its dates as an occurrence, since its dates share
/// one cause: its model calendar. The file is named by its `source`, so a downloaded one is named by
/// its URL, the same in every run, rather than by the temporary copy it was read from.
pub(crate) fn report_nonexistent_dates(sink: &dyn DiagnosticSink, source: &Input, variables: &[NonexistentDates]) -> u64 {
    variables
        .iter()
        .map(|dates| {
            sink.report(
                &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Ingest(IngestCode::DatesNonexistent), headline(source, dates))
                    // The diagnostic outlives the ingestor that names the source.
                    .with_context(ContextField::Source(source.clone()))
                    .with_cause(cause(dates))
                    .with_occurrences(dates.count)
                    .build(),
            );
            dates.count.get()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use crate::{stages::nonexistent_dates_report::report_nonexistent_dates, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::netcdf::{calendar::Calendar, nonexistent_dates::NonexistentDates, variable_name::VariableName};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU64, path::PathBuf};

    fn dates(variable: &str, calendar: Calendar, first: &str, count: u64) -> NonexistentDates {
        NonexistentDates {
            variable: VariableName::from(variable),
            calendar,
            first: first.to_owned(),
            count: NonZeroU64::new(count).unwrap(),
        }
    }

    fn source() -> Input {
        Input::Local(PathBuf::from("/data/tas.nc"))
    }

    #[test]
    fn a_file_without_nonexistent_dates_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_nonexistent_dates(&sink, &source(), &[]), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_nonexistent_date_is_one_warning_naming_the_variable_the_file_the_calendar_and_the_date() {
        let sink = RecordingReporter::new();

        let total = report_nonexistent_dates(&sink, &source(), &[dates("time", Calendar::AllLeap, "2026-02-29T00:00:00+00:00", 1)]);

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::DatesNonexistent));
        assert_eq!(reported[0].code.to_string(), "ingest-dates-nonexistent");
        assert_eq!(
            reported[0].headline,
            "The time variable `time` of `/data/tas.nc` holds a date of the `all_leap` calendar that the Gregorian calendar does not have, `2026-02-29T00:00:00+00:00`, so the records keep it as labelled"
        );
        assert_eq!(
            reported[0].causes,
            [
                "a day of the `all_leap` calendar of `time` that the Gregorian calendar does not have names no instant, so a `datetime` transformation cannot read its date"
            ]
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].defining, [ContextField::Source(source())]);
    }

    #[test]
    fn each_variable_is_its_own_warning_counting_its_dates_and_naming_the_first() {
        let sink = RecordingReporter::new();

        let total = report_nonexistent_dates(
            &sink,
            &source(),
            &[
                dates("time", Calendar::Day360, "2026-02-29T00:00:00+00:00", 200),
                dates("forecast_reference_time", Calendar::Day360, "2026-02-30T00:00:00+00:00", 3),
            ],
        );

        let reported = sink.diagnostics();
        assert_eq!(total, 203);
        assert_eq!(reported.len(), 2);
        assert_eq!(
            reported[0].headline,
            "The time variable `time` of `/data/tas.nc` holds 200 dates of the `360_day` calendar that the Gregorian calendar does not have, the first `2026-02-29T00:00:00+00:00`, so the records keep them as labelled"
        );
        assert_eq!(reported[0].occurrences, 200);
        assert_eq!(reported[1].occurrences, 3);
        assert!(
            reported[1]
                .headline
                .starts_with("The time variable `forecast_reference_time` of `/data/tas.nc` holds 3 dates")
        );
        assert_ne!(reported[0].causes, reported[1].causes);
    }
}

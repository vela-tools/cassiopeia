use crate::backend::terminal::summary::thousands::thousands;
use anstyle::Style;
use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_terminal_style::{
    byte_size::human_size,
    paint::paint,
    palette::{CAUTION, ERROR, FRAME, SUCCESS},
    rendering::Rendering,
};
use std::time::Duration;

/// The label column's width, so two cells on one line sit in aligned columns.
const LABEL_WIDTH: usize = 17;

/// The value column's width, so a value is right-aligned against its label whatever its magnitude.
const VALUE_WIDTH: usize = 11;

/// What a counter means for the run's health, which decides the colour its value carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CounterKind {
    /// A throughput or resource figure: green whatever it reads.
    Neutral,
    /// A warning tally: amber once anything was warned.
    Warnings,
    /// An error tally: red once anything failed.
    Errors,
}

/// The run's headline totals, two cells to a line.
pub(crate) fn counters_block(snapshot: &TelemetrySnapshot, rendering: Rendering) -> Vec<String> {
    let counters = snapshot.counters;
    vec![
        pair(
            &neutral("input records", &thousands(counters.input_records), rendering),
            &neutral("fragments created", &thousands(counters.fragments_created), rendering),
        ),
        pair(
            &neutral("unique entities", &thousands(counters.unique_entities), rendering),
            &neutral("entities written", &thousands(counters.entities_written), rendering),
        ),
        pair(
            &counter_cell(
                "errors",
                &thousands(counters.errors),
                counter_style(CounterKind::Errors, counters.errors),
                rendering,
            ),
            &counter_cell(
                "warnings",
                &thousands(counters.warnings),
                counter_style(CounterKind::Warnings, counters.warnings),
                rendering,
            ),
        ),
        pair(
            &neutral("peak memory", &human_size(snapshot.memory.peak_bytes), rendering),
            &neutral("avg memory", &human_size(snapshot.memory.average_bytes), rendering),
        ),
        pair(
            &neutral("cpu time", &format!("{:.1}s", snapshot.cpu.cpu_time.as_secs_f64()), rendering),
            &neutral(
                "cpu util",
                &format!("{:.0}%", cpu_utilization(snapshot.cpu.cpu_time, snapshot.elapsed)),
                rendering,
            ),
        ),
        format!("  {}", neutral("elapsed", &format!("{:.1}s", snapshot.elapsed.as_secs_f64()), rendering)),
    ]
}

/// A counter whose value says nothing about the run's health.
fn neutral(label: &str, value: &str, rendering: Rendering) -> String {
    counter_cell(label, value, counter_style(CounterKind::Neutral, 0), rendering)
}

/// Lays two counter cells on one indented line.
fn pair(left: &str, right: &str) -> String {
    format!("  {left}  {right}")
}

/// The colour a counter's value carries: a tally that counts trouble only turns colour once there is
/// trouble to count, so a clean run reads green throughout.
pub(crate) const fn counter_style(kind: CounterKind, value: u64) -> Style {
    match kind {
        CounterKind::Warnings if value > 0 => CAUTION,
        CounterKind::Errors if value > 0 => ERROR,
        CounterKind::Neutral | CounterKind::Warnings | CounterKind::Errors => SUCCESS,
    }
}

/// Renders one dim label and its value as a fixed-width `input records      1,000,000` cell.
pub(crate) fn counter_cell(label: &str, value: &str, style: Style, rendering: Rendering) -> String {
    format!(
        "{} {}",
        paint(FRAME, &format!("{label:<LABEL_WIDTH$}"), rendering),
        paint(style, &format!("{value:>VALUE_WIDTH$}"), rendering)
    )
}

/// Returns CPU time as a percentage of wall time. It is uncapped: a parallel run legitimately
/// consumes more than one CPU-second per wall-second, so a figure above 100% is meaningful.
pub(crate) fn cpu_utilization(cpu: Duration, wall: Duration) -> f64 {
    let wall = wall.as_secs_f64();
    if wall > 0.0 { cpu.as_secs_f64() / wall * 100.0 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::summary::counters_block::{CounterKind, counter_cell, counter_style, cpu_utilization};
    use cassiopeia_terminal_style::{
        palette::{CAUTION, ERROR, SUCCESS},
        rendering::Rendering,
    };
    use std::time::Duration;

    #[test]
    fn a_clean_tally_stays_green_and_a_dirty_one_turns_colour() {
        assert_eq!(counter_style(CounterKind::Errors, 0), SUCCESS);
        assert_eq!(counter_style(CounterKind::Warnings, 0), SUCCESS);
        assert_eq!(counter_style(CounterKind::Errors, 142), ERROR);
        assert_eq!(counter_style(CounterKind::Warnings, 18), CAUTION);
        assert_eq!(counter_style(CounterKind::Neutral, 0), SUCCESS);
        assert_eq!(counter_style(CounterKind::Neutral, 999), SUCCESS);
    }

    #[test]
    fn a_plain_counter_cell_pads_the_label_and_right_aligns_the_value() {
        assert_eq!(
            counter_cell("input records", "1,000,000", SUCCESS, Rendering::Plain),
            "input records       1,000,000"
        );
    }

    #[test]
    fn cpu_utilization_exceeds_one_hundred_percent_under_parallelism() {
        assert!((cpu_utilization(Duration::from_secs(24), Duration::from_secs(10)) - 240.0).abs() < 1e-6);
        assert!(cpu_utilization(Duration::from_secs(5), Duration::ZERO).abs() < f64::EPSILON);
    }
}

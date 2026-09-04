use crate::backend::terminal::summary::{
    table_layout::{dim_header, header_row},
    thousands::thousands,
};
use cassiopeia_common::telemetry::stage_metrics::StageSnapshot;
use cassiopeia_terminal_style::rendering::Rendering;
use std::time::Duration;
use tabular::{Row, Table};

/// Renders the per-stage throughput table: label, completed count, average and live throughput, the
/// service time share and how much of it was CPU, the input-wait/output-wait time shares, and wall
/// time. The `cpu` column is the stage's CPU time as a share of its wall time, so comparing it with
/// `service` shows how much of the service span was compute rather than synchronous stalls.
pub(crate) fn stages_table(stages: &[StageSnapshot], rendering: Rendering) -> Vec<String> {
    let mut table = Table::new("  {:<}  {:>}  {:>}  {:>}  {:>}  {:>}  {:>}  {:>}  {:>}");
    table.add_row(header_row(&[
        "stage",
        "completed",
        "avg/s",
        "live/s",
        "service",
        "cpu",
        "in-wait",
        "out-wait",
        "wall",
    ]));
    for stage in stages {
        table.add_row(
            Row::new()
                .with_cell(stage.stage.label())
                .with_cell(thousands(stage.completed))
                .with_cell(format!("{:.1}", stage.rates.average_throughput))
                .with_cell(format!("{:.1}", stage.rates.live_throughput))
                .with_cell(format!("{:.0}%", share(stage.service_time, stage.wall_time)))
                .with_cell(format!("{:.0}%", share(stage.service_cpu, stage.wall_time)))
                .with_cell(format!("{:.0}%", share(stage.input_wait, stage.wall_time)))
                .with_cell(format!("{:.0}%", share(stage.output_wait, stage.wall_time)))
                .with_cell(format!("{:.1}s", stage.wall_time.as_secs_f64())),
        );
    }
    dim_header(&table, rendering)
}

/// Returns `part` as a percentage of `whole`, capped at 100 and zero when `whole` is zero.
fn share(part: Duration, whole: Duration) -> f64 {
    let whole = whole.as_secs_f64();
    if whole > 0.0 { (part.as_secs_f64() / whole * 100.0).min(100.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::summary::stages_table::share;
    use std::time::Duration;

    #[test]
    fn a_share_caps_at_one_hundred_percent() {
        assert!((share(Duration::from_secs(5), Duration::from_secs(10)) - 50.0).abs() < f64::EPSILON);
        assert!((share(Duration::from_secs(30), Duration::from_secs(10)) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_share_of_no_wall_time_is_zero() {
        assert!(share(Duration::from_secs(5), Duration::ZERO).abs() < f64::EPSILON);
    }
}

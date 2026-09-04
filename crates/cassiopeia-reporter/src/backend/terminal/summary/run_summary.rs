use crate::backend::terminal::summary::{
    channels_table::channels_table,
    counters_block::counters_block,
    reasons_table::reasons_table,
    stages_table::stages_table,
};
use cassiopeia_common::telemetry::{channel_metrics::ChannelSnapshot, run::TelemetrySnapshot};
use cassiopeia_diagnostic::reason::Reason;
use cassiopeia_terminal_style::{paint::paint, palette::ACCENT, rendering::Rendering};
use std::cmp::Reverse;

/// Builds the full run summary as a list of ready-to-print lines.
pub(crate) fn render(snapshot: &TelemetrySnapshot, reasons: &[Reason], rendering: Rendering) -> Vec<String> {
    let mut lines = vec![String::new(), paint(ACCENT, "Run summary", rendering)];
    lines.extend(counters_block(snapshot, rendering));

    if !snapshot.stages.is_empty() {
        lines.push(String::new());
        lines.push(paint(ACCENT, "Stages", rendering));
        lines.extend(stages_table(&snapshot.stages, rendering));
    }

    // Idle channels (nothing ever sent) carry no metrics worth a row, so they are dropped rather
    // than filling the table with zeroes; the rest are ordered by traffic so the busy queues lead.
    let mut active: Vec<&ChannelSnapshot> = snapshot.channels.iter().filter(|channel| channel.sent > 0).collect();
    active.sort_by_key(|channel| Reverse(channel.sent));
    if !active.is_empty() {
        lines.push(String::new());
        lines.push(paint(ACCENT, "Channels", rendering));
        lines.extend(channels_table(&active, rendering));
    }

    // The reason table exists to explain a non-zero counter, so it is drawn only when there is one
    // and only when something was actually recorded to explain it.
    let counters = snapshot.counters;
    if counters.errors + counters.warnings > 0 && !reasons.is_empty() {
        lines.push(String::new());
        lines.push(paint(ACCENT, "Reasons", rendering));
        lines.extend(reasons_table(reasons, rendering));
    }

    lines
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::summary::run_summary::render;
    use cassiopeia_common::telemetry::run::{RunTelemetry, TelemetrySnapshot};
    use cassiopeia_diagnostic::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode},
        reason::Reason,
        severity::Severity,
    };
    use cassiopeia_terminal_style::rendering::Rendering;

    /// A snapshot of a run that counted `errors` failures and nothing else.
    fn snapshot(errors: u64) -> TelemetrySnapshot {
        let telemetry = RunTelemetry::new();
        telemetry.add_errors(errors);
        telemetry.snapshot()
    }

    fn reasons() -> Vec<Reason> {
        vec![Reason::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::EntityRejected),
            142,
            "attribute 'dateObserved' is not a valid DateTime".into(),
        )]
    }

    #[test]
    fn a_clean_run_renders_no_reason_block() {
        let lines = render(&snapshot(0), &[], Rendering::Plain);

        assert!(!lines.iter().any(|line| line == "Reasons"));
    }

    #[test]
    fn a_failing_run_with_recorded_reasons_renders_the_block() {
        let lines = render(&snapshot(142), &reasons(), Rendering::Plain);

        assert!(lines.iter().any(|line| line == "Reasons"));
        assert!(lines.iter().any(|line| line.contains("broker-entity-rejected")));
    }

    #[test]
    fn a_failing_run_that_recorded_nothing_renders_no_reason_block() {
        let lines = render(&snapshot(142), &[], Rendering::Plain);

        assert!(!lines.iter().any(|line| line == "Reasons"));
    }

    #[test]
    fn the_reason_block_follows_every_other_block() {
        let lines = render(&snapshot(142), &reasons(), Rendering::Plain);
        let reasons_at = lines.iter().position(|line| line == "Reasons").expect("a reason block");
        let summary_at = lines.iter().position(|line| line == "Run summary").expect("a summary block");

        assert!(reasons_at > summary_at);
        assert_eq!(reasons_at, lines.len() - 3);
    }
}

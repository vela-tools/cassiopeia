use crate::backend::terminal::summary::{
    table_layout::{dim_header, header_row},
    thousands::thousands,
};
use cassiopeia_common::{
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, channel_metrics::ChannelSnapshot},
};
use cassiopeia_terminal_style::rendering::Rendering;
use tabular::{Row, Table};

/// Renders the per-channel queue table: the stage boundary the queue bridges, its capacity, deepest
/// backlog, time spent at capacity, blocked sends, and the sent/received totals.
pub(crate) fn channels_table(channels: &[&ChannelSnapshot], rendering: Rendering) -> Vec<String> {
    let mut table = Table::new("  {:<}  {:>}  {:>}  {:>}  {:>}  {:>}  {:>}");
    table.add_row(header_row(&["queue", "cap", "peak", "full", "blocked", "sent", "received"]));
    for channel in channels {
        let capacity = channel.capacity.map_or_else(|| "none".to_string(), |capacity| capacity.to_string());
        table.add_row(
            Row::new()
                .with_cell(channel_label(channel.boundary))
                .with_cell(capacity)
                .with_cell(thousands(u64::try_from(channel.high_water_depth).unwrap_or(u64::MAX)))
                .with_cell(format!("{:.2}s", channel.time_at_capacity.as_secs_f64()))
                .with_cell(thousands(channel.blocked_sends))
                .with_cell(thousands(channel.sent))
                .with_cell(thousands(channel.received)),
        );
    }
    dim_header(&table, rendering)
}

/// Names the queue by the stage boundary it sits on, matching the labels used in the Stages table.
/// A queue whose consumer drains to the run's terminal sink shows `-> output`; an unlabelled internal
/// handoff shows `internal`.
fn channel_label(boundary: Option<ChannelBoundary>) -> String {
    match boundary {
        Some(boundary) => format!("{} -> {}", boundary.producer.label(), boundary.consumer.map_or("output", Stage::label)),
        None => "internal".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::summary::channels_table::channel_label;
    use cassiopeia_common::{stage::Stage, telemetry::channel_boundary::ChannelBoundary};

    #[test]
    fn channel_labels_name_the_stage_boundary() {
        assert_eq!(
            channel_label(Some(ChannelBoundary::between(Stage::Ingestor, Stage::Expander))),
            "Ingestor -> Expander"
        );
        assert_eq!(
            channel_label(Some(ChannelBoundary::between(Stage::Assembler, Stage::Extractor))),
            "Assembler -> Extractor"
        );
        assert_eq!(
            channel_label(Some(ChannelBoundary::between(Stage::Extractor, Stage::Transformer))),
            "Extractor -> Transformer"
        );
        assert_eq!(channel_label(Some(ChannelBoundary::to_sink(Stage::Writer))), "Writer -> output");
        assert_eq!(channel_label(None), "internal");
    }
}

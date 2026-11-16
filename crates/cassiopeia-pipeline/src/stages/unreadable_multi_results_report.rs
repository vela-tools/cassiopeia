use crate::stages::source_warning::report_source_warning;
use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::code::ingest_code::IngestCode;
use cassiopeia_ingestor::sensorthings::ingest_losses::{LossTally, UnreadableMultiResult};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// The one-line summary: the source, how many Observations of `MultiDatastreams` gave no readings
/// for their `result`, and the first such Observation with its `MultiDatastream`.
fn headline(source: &Input, first: &UnreadableMultiResult, total: NonZeroU64) -> String {
    match total.get() {
        1 => format!(
            "In `{source}`, 1 Observation of a MultiDatastream has a `result` that is no array with one value per ObservedProperty, so it gives no readings: {first}"
        ),
        count => format!(
            "In `{source}`, {count} Observations of MultiDatastreams have a `result` that is no array with one value per ObservedProperty, so they give no readings: {first}, and {} more",
            count.saturating_sub(1)
        ),
    }
}

/// Warns once for a SensorThings snapshot whose `MultiDatastream` Observations have a `result` that
/// cannot be read by position, and returns how many Observations that was.
///
/// One diagnostic per snapshot, carrying every such Observation as an occurrence. Each value of a
/// `MultiDatastream` result belongs to the `ObservedProperty` at its position (OGC 18-088 section
/// 12), so a result of another length names no reading. The snapshot is named by its `source`, so a
/// downloaded one is named by its URL.
pub(crate) fn report_unreadable_multi_results(sink: &dyn DiagnosticSink, source: &Input, results: &LossTally<UnreadableMultiResult>) -> u64 {
    let (Some(total), Some(first)) = (NonZeroU64::new(results.total()), results.named().first()) else {
        return 0;
    };
    report_source_warning(sink, source, IngestCode::MultiResultsUnreadable, headline(source, first, total), total)
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_multi_results_report::report_unreadable_multi_results, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::sensorthings::ingest_losses::{ObservationName, SensorThingsLosses, UnreadableMultiResult};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    /// The losses of a snapshot whose Observations `observations` of `MultiDatastream` 4 had results
    /// of another length than its `ObservedProperties`.
    fn losses(observations: &[i64]) -> SensorThingsLosses {
        let mut losses = SensorThingsLosses::default();
        for &observation in observations {
            losses.unreadable_multi_result(UnreadableMultiResult {
                observation: ObservationName::Id(EntityId::Integer(observation)),
                multi_datastream: EntityId::Integer(4),
            });
        }
        losses
    }

    fn source() -> Input {
        Input::Local(PathBuf::from("/data/wind.jsonl"))
    }

    #[test]
    fn readable_results_say_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_unreadable_multi_results(&sink, &source(), losses(&[]).unreadable_multi_results()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_unreadable_result_names_its_observation_and_multi_datastream() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_multi_results(&sink, &source(), losses(&[501]).unreadable_multi_results());

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::MultiResultsUnreadable));
        assert_eq!(reported[0].code.to_string(), "ingest-multi-results-unreadable");
        assert_eq!(
            reported[0].headline,
            "In `/data/wind.jsonl`, 1 Observation of a MultiDatastream has a `result` that is no array with one value per ObservedProperty, so it gives no readings: Observation 501 of MultiDatastream 4"
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].defining, [ContextField::Source(source())]);
    }

    #[test]
    fn several_unreadable_results_name_the_first_and_count_every_one() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_multi_results(&sink, &source(), losses(&[501, 502, 503]).unreadable_multi_results());

        assert_eq!(total, 3);
        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `/data/wind.jsonl`, 3 Observations of MultiDatastreams have a `result` that is no array with one value per ObservedProperty, so they give no readings: Observation 501 of MultiDatastream 4, and 2 more"
        );
    }
}

use crate::stages::{batch_sink_report::report_sink, unreadable_timestamp_report::unreadable_timestamp_diagnostic};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_resolver::unreadable_observations::UnreadableObservations;
use cassiopeia_unreadable_timestamps::{timestamp_loss::TimestampLoss, unreadable_timestamps::UnreadableTimestamps};

/// Names the unreadable `observedAt` text of the records a current-state store discarded, and
/// returns how many losses that is.
///
/// A discarded record never reaches the extractor or the transformer, which report the same text
/// when its record is the one kept. Each declaration that reads the text is named here with the
/// loss the record would have cost it had it been kept (see
/// [`TimestampLoss::of_unreadable_observed_at`]), so unreadable text reads the same in a report
/// whichever record of an entity won its ranking, under the same code and on the same line.
pub(crate) fn report_unreadable_observations(sink: &dyn DiagnosticSink, unreadable: UnreadableObservations) -> u64 {
    if unreadable.is_empty() {
        return 0;
    }

    let timestamps = UnreadableTimestamps::new();
    for entry in unreadable.into_entries() {
        for declaration in entry.declarations() {
            let (loss, attribute) =
                TimestampLoss::of_unreadable_observed_at(declaration.conversion.temporal_reading(), &declaration.observed_at, &declaration.attribute);
            timestamps.record_occurrences(loss, attribute, entry.example.as_str(), entry.records);
        }
    }
    report_sink(sink, timestamps, unreadable_timestamp_diagnostic)
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_observation_report::report_unreadable_observations, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode, transform_code::TransformCode};
    use cassiopeia_mapping::{mapping::Mapping, observed_at::ObservedAt, observed_template::ObservedTemplateIndex, template::runner::TemplateRunner};
    use cassiopeia_resolver::{entity_store::discarded_timestamps::UnreadableDiscards, mapping_id::MappingId, unreadable_observations::UnreadableObservations};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU64, path::Path, sync::Arc};

    /// A mapping whose `reading` reads `time` as a date-time and whose `status` keeps it as written.
    const SENSOR: &str = r#"{
        version: "v4",
        dataModel: "Sensor",
        identity: { entityName: "{{ code }}" },
        attributes: {
            reading: { source: "{{ reading }}", properties: { observedAt: { source: "{{ time }}", transformation: "datetime" } } },
            status: { source: "{{ status }}", properties: { observedAt: { source: "{{ time }}" } } },
        },
    }"#;

    fn unreadable(records: u64) -> UnreadableObservations {
        let mapping = Arc::new(Mapping::from_json5(SENSOR, Path::new("sensor.json5"), &mut TemplateRunner::new()).unwrap());
        let mut unreadable = UnreadableObservations::new();
        unreadable.record(
            MappingId::new(1),
            &mapping,
            &UnreadableDiscards {
                template: ObservedTemplateIndex::new(0),
                example: ObservedAt::new("soon"),
                records: NonZeroU64::new(records).unwrap(),
            },
        );
        unreadable
    }

    #[test]
    fn nothing_discarded_reports_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_unreadable_observations(&sink, UnreadableObservations::new()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn each_declaration_reading_the_text_is_named_with_the_loss_a_kept_record_would_have_cost_it() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_observations(&sink, unreadable(3));

        let reported = sink.diagnostics();
        assert_eq!(total, 6);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable));
        assert_eq!(
            reported[0].headline,
            "Attribute `observedAt` dropped: `soon` is not a date-time in any spelling Cassiopeia reads"
        );
        assert_eq!(reported[0].occurrences, 3);
        assert_eq!(reported[1].code, DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable));
        assert_eq!(
            reported[1].headline,
            "Attribute `status` emitted without `observedAt`: `soon` is not a date-time in any spelling Cassiopeia reads"
        );
        assert_eq!(reported[1].occurrences, 3);
    }
}

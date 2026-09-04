use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode, transform_code::TransformCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_unreadable_timestamps::{unreadable_timestamp::UnreadableTimestamp, unreadable_timestamps::UnreadableTimestamps};

/// What an attribute lost because a timestamp it carried could not be read.
///
/// The two losses look the same to a mapping author (a source spelling Cassiopeia does not read),
/// but they cost different things and are met by different stages, so each publishes its own code.
#[derive(Clone, Copy)]
pub(crate) enum TimestampLoss {
    /// The attribute's own value, so no trace of the attribute is emitted at all.
    AttributeValue,
    /// The attribute's `observedAt` qualifier, so the value is emitted with nothing anchoring it in
    /// time (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    ObservedAt,
}

impl TimestampLoss {
    /// The code this loss is published under.
    const fn code(self) -> DiagnosticCode {
        match self {
            TimestampLoss::AttributeValue => DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable),
            TimestampLoss::ObservedAt => DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable),
        }
    }

    /// The one-line summary, naming the attribute, what it lost, and the text it lost it to.
    ///
    /// The headline carries all three because the concise form shows nothing else: the attribute
    /// travels as a context field, but that field is only rendered under `-v`, and a reader who
    /// cannot see which attribute and which spelling has nothing to go and fix.
    fn headline(self, attribute: &NameBuf, record: &UnreadableTimestamp) -> String {
        let lost = match self {
            TimestampLoss::AttributeValue => "dropped",
            TimestampLoss::ObservedAt => "emitted without `observedAt`",
        };

        format!(
            "Attribute `{attribute}` {lost}: `{}` is not a date-time in any spelling Cassiopeia reads",
            record.example
        )
    }
}

/// Names each attribute's unreadable timestamps once and returns how many records lost one.
///
/// One diagnostic per attribute, however many records the attribute lost a timestamp on and however
/// many distinct spellings they carried: the attribute and the code are the failure, and the example
/// spelling rides on the headline, which stays outside the diagnostic's identity so it cannot split
/// one group into a line per batch.
pub(crate) fn report_unreadable_timestamps(sink: &dyn DiagnosticSink, loss: TimestampLoss, unreadable: UnreadableTimestamps) -> u64 {
    if unreadable.is_empty() {
        return 0;
    }

    let mut total: u64 = 0;
    for (attribute, record) in unreadable.into_entries() {
        sink.report(
            &DiagnosticBuilder::new(Severity::Warning, loss.code(), loss.headline(&attribute, &record))
                .with_context(ContextField::Attribute(attribute))
                .with_occurrences(record.occurrences)
                .build(),
        );
        total = total.saturating_add(record.occurrences.get());
    }

    total
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::unreadable_timestamp_report::{TimestampLoss, report_unreadable_timestamps},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode, transform_code::TransformCode};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn an_empty_sink_says_nothing_and_counts_nothing() {
        assert_eq!(
            report_unreadable_timestamps(&NoopReporter::new(), TimestampLoss::AttributeValue, UnreadableTimestamps::new()),
            0
        );
    }

    #[test]
    fn one_attribute_is_named_once_however_many_spellings_it_lost() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(&name("dateObserved"), "2026-03-01 11:04:35+00:00");
        unreadable.record(&name("dateObserved"), "2026-03-01 11:04:36+00:00");
        unreadable.record(&name("dateObserved"), "2026-03-01 11:04:37+00:00");

        let total = report_unreadable_timestamps(&sink, TimestampLoss::AttributeValue, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 3);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 3);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable));
        assert!(reported[0].headline.contains("dateObserved"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("2026-03-01 11:04:35+00:00"), "{}", reported[0].headline);
    }

    #[test]
    fn a_lost_observed_at_is_published_under_its_own_code_and_says_what_it_cost() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(&name("temperature"), "not a timestamp");

        report_unreadable_timestamps(&sink, TimestampLoss::ObservedAt, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(reported[0].code, DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable));
        assert!(reported[0].headline.contains("observedAt"), "{}", reported[0].headline);
    }

    #[test]
    fn two_attributes_are_two_diagnostics_in_the_order_they_failed() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(&name("dateObservedTo"), "later");
        unreadable.record(&name("dateObserved"), "earlier");

        let total = report_unreadable_timestamps(&sink, TimestampLoss::AttributeValue, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 2);
        assert!(reported[0].headline.contains("dateObservedTo"));
        assert!(reported[1].headline.contains("dateObserved"));
    }
}

use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode, transform_code::TransformCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_unreadable_timestamps::{timestamp_loss::TimestampLoss, unreadable_timestamp::UnreadableTimestamp, unreadable_timestamps::UnreadableTimestamps};

/// The code the loss of one attribute timestamp is published under.
///
/// The three losses look the same to a mapping author (a source spelling Cassiopeia does not read),
/// but they cost different things and are met by different stages, so each publishes its own code.
const fn code(loss: TimestampLoss) -> DiagnosticCode {
    match loss {
        TimestampLoss::AttributeValue => DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable),
        TimestampLoss::ObservedAt => DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable),
        TimestampLoss::ExpiresAt => DiagnosticCode::Transform(TransformCode::ExpiresAtUnreadable),
    }
}

/// The one-line summary, naming the attribute, what it lost, and the text it lost it to.
///
/// The headline carries all three because the concise form shows nothing else: the attribute
/// travels as a context field, but that field is only rendered under `-v`, and a reader who
/// cannot see which attribute and which spelling has nothing to go and fix.
fn headline(loss: TimestampLoss, attribute: &NameBuf, record: &UnreadableTimestamp) -> String {
    let cost = match loss {
        TimestampLoss::AttributeValue => "dropped",
        TimestampLoss::ObservedAt => "emitted without `observedAt`",
        TimestampLoss::ExpiresAt => "emitted without `expiresAt`",
    };

    format!(
        "Attribute `{attribute}` {cost}: `{}` is not a date-time in any spelling Cassiopeia reads",
        record.example
    )
}

/// Names each attribute's unreadable timestamps once and returns how many records lost one.
///
/// One diagnostic per attribute timestamp lost, however many records lost it and however many
/// distinct spellings they carried: the attribute, the loss, and the code are the failure, and the
/// example spelling rides on the headline, which stays outside the diagnostic's identity so it
/// cannot split one group into a line per batch.
pub(crate) fn report_unreadable_timestamps(sink: &dyn DiagnosticSink, unreadable: UnreadableTimestamps) -> u64 {
    if unreadable.is_empty() {
        return 0;
    }

    let mut total: u64 = 0;
    for ((loss, attribute), record) in unreadable.into_entries() {
        sink.report(
            &DiagnosticBuilder::new(Severity::Warning, code(loss), headline(loss, &attribute, &record))
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
    use crate::{stages::unreadable_timestamp_report::report_unreadable_timestamps, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode, transform_code::TransformCode};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use cassiopeia_unreadable_timestamps::{timestamp_loss::TimestampLoss, unreadable_timestamps::UnreadableTimestamps};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn an_empty_sink_says_nothing_and_counts_nothing() {
        assert_eq!(report_unreadable_timestamps(&NoopReporter::new(), UnreadableTimestamps::new()), 0);
    }

    #[test]
    fn one_attribute_is_named_once_however_many_spellings_it_lost() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(TimestampLoss::AttributeValue, &name("dateObserved"), "2026-03-01 11:04:35+00:00");
        unreadable.record(TimestampLoss::AttributeValue, &name("dateObserved"), "2026-03-01 11:04:36+00:00");
        unreadable.record(TimestampLoss::AttributeValue, &name("dateObserved"), "2026-03-01 11:04:37+00:00");

        let total = report_unreadable_timestamps(&sink, unreadable);

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
        unreadable.record(TimestampLoss::ObservedAt, &name("temperature"), "not a timestamp");

        report_unreadable_timestamps(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(reported[0].code, DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable));
        assert!(reported[0].headline.contains("observedAt"), "{}", reported[0].headline);
    }

    #[test]
    fn a_lost_expires_at_is_published_under_its_own_code_and_says_what_it_cost() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(TimestampLoss::ExpiresAt, &name("until"), "the day after tomorrow");

        report_unreadable_timestamps(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(reported[0].code, DiagnosticCode::Transform(TransformCode::ExpiresAtUnreadable));
        assert!(reported[0].headline.contains("expiresAt"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("until"), "{}", reported[0].headline);
    }

    #[test]
    fn two_losses_on_one_attribute_are_two_diagnostics_under_their_own_codes() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(TimestampLoss::ExpiresAt, &name("temperature"), "later");
        unreadable.record(TimestampLoss::ObservedAt, &name("temperature"), "earlier");

        report_unreadable_timestamps(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].code, DiagnosticCode::Transform(TransformCode::ExpiresAtUnreadable));
        assert_eq!(reported[1].code, DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable));
    }

    #[test]
    fn two_attributes_are_two_diagnostics_in_the_order_they_failed() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(TimestampLoss::AttributeValue, &name("dateObservedTo"), "later");
        unreadable.record(TimestampLoss::AttributeValue, &name("dateObserved"), "earlier");

        let total = report_unreadable_timestamps(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 2);
        assert!(reported[0].headline.contains("dateObservedTo"));
        assert!(reported[1].headline.contains("dateObserved"));
    }
}

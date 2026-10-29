use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode},
    context_field::{ContextField, minted_id},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_unreadable_timestamps::{unreadable_expiries::UnreadableExpiries, unreadable_timestamp::UnreadableTimestamp};
use urn_rs::Urn;

/// The one-line summary, naming the entity and the text its expiry was lost to.
///
/// The headline carries both because the concise form shows nothing else: the entity travels as a
/// context field, but that field is only rendered under `-v`, and a reader who cannot see which
/// entity and which spelling has nothing to go and fix.
fn headline(entity: &Urn, record: &UnreadableTimestamp) -> String {
    format!(
        "Entity `{entity}` publishes without `expiresAt`: `{}` is not a date-time in any spelling Cassiopeia reads",
        record.example
    )
}

/// Names each entity's unreadable expiry once and returns how many records lost one.
///
/// One diagnostic per entity whose `expiresAt` was lost, however many records lost it and however
/// many distinct spellings they carried: the entity and the code are the failure, and the example
/// spelling rides on the headline, which stays outside the diagnostic's identity so it cannot split
/// one group into a line per batch. The member an entity loses is the one its records declared at
/// the top level (ETSI GS CIM 009 v1.9.1 clause 4.5.1), so the code is the expander's, the stage
/// that resolves it.
pub(crate) fn report_unreadable_expiries(sink: &dyn DiagnosticSink, unreadable: UnreadableExpiries) -> u64 {
    if unreadable.is_empty() {
        return 0;
    }

    let mut total: u64 = 0;
    for (entity, record) in unreadable.into_entries() {
        let builder = DiagnosticBuilder::new(
            Severity::Warning,
            DiagnosticCode::Expander(ExpanderCode::ExpiryUnreadable),
            headline(&entity, &record),
        );
        let builder = match minted_id(&entity) {
            Some(first) => builder.with_context(ContextField::Entities { first, additional: 0 }),
            None => builder,
        };
        sink.report(&builder.with_occurrences(record.occurrences).build());
        total = total.saturating_add(record.occurrences.get());
    }

    total
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_expiry_report::report_unreadable_expiries, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode};
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use cassiopeia_unreadable_timestamps::unreadable_expiries::UnreadableExpiries;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    #[test]
    fn an_empty_sink_says_nothing_and_counts_nothing() {
        assert_eq!(report_unreadable_expiries(&NoopReporter::new(), UnreadableExpiries::new()), 0);
    }

    #[test]
    fn one_entity_is_named_once_however_many_records_lost_their_expiry() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableExpiries::new();
        unreadable.record(&urn("urn:ngsi-ld:Sensor:1"), "the day after tomorrow");
        unreadable.record(&urn("urn:ngsi-ld:Sensor:1"), "sometime next week");

        let total = report_unreadable_expiries(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Expander(ExpanderCode::ExpiryUnreadable));
        assert!(reported[0].headline.contains("urn:ngsi-ld:Sensor:1"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("the day after tomorrow"), "{}", reported[0].headline);
    }

    #[test]
    fn two_entities_are_two_diagnostics_in_the_order_they_failed() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableExpiries::new();
        unreadable.record(&urn("urn:ngsi-ld:Sensor:2"), "later");
        unreadable.record(&urn("urn:ngsi-ld:Sensor:1"), "earlier");

        let total = report_unreadable_expiries(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 2);
        assert!(reported[0].headline.contains("urn:ngsi-ld:Sensor:2"));
        assert!(reported[1].headline.contains("urn:ngsi-ld:Sensor:1"));
    }
}

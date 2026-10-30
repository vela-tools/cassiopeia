use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode},
    context_field::{ContextField, minted_id},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_unreadable_timestamps::{unreadable_expiries::UnreadableExpiries, unreadable_expiry::UnreadableExpiry};

/// The one-line summary, naming the entity type, how many records lost their expiry, the first
/// entity that did, and the text it was lost to.
///
/// The headline carries all of them because the concise form shows nothing else: the type and the
/// entity travel as context fields, but those are only rendered under `-v`, and a reader who cannot
/// see which mapping, which record, and which spelling has nothing to go and fix.
fn headline(entity_type: &NameBuf, record: &UnreadableExpiry) -> String {
    format!(
        "{} `{entity_type}` records publish without `expiresAt` (first: `{}`): `{}` is not a date-time in any spelling Cassiopeia reads",
        record.unreadable.occurrences, record.exemplar, record.unreadable.example
    )
}

/// Names each entity type's unreadable expiries once and returns how many records lost one.
///
/// One diagnostic per entity type whose `expiresAt` was lost, however many entities lost it and
/// however many distinct spellings they carried: the type and the code are the failure, the first
/// entity rides on the headline and as an incidental context field, and the example spelling rides
/// on the headline, which stays outside the diagnostic's identity so it cannot split one group into
/// a line per batch. The member an entity loses is the one its records declared at the top level
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.1), so the code is the expander's, the stage that resolves it.
pub(crate) fn report_unreadable_expiries(sink: &dyn DiagnosticSink, unreadable: UnreadableExpiries) -> u64 {
    if unreadable.is_empty() {
        return 0;
    }

    let mut total: u64 = 0;
    for (entity_type, record) in unreadable.into_entries() {
        let occurrences = record.unreadable.occurrences;
        let entities = minted_id(&record.exemplar).map(|first| ContextField::Entities {
            first,
            additional: occurrences.get() - 1,
        });
        sink.report(
            &DiagnosticBuilder::new(
                Severity::Warning,
                DiagnosticCode::Expander(ExpanderCode::ExpiryUnreadable),
                headline(&entity_type, &record),
            )
            .with_context(ContextField::EntityType(entity_type))
            .with_optional_context(entities)
            .with_occurrences(occurrences)
            .build(),
        );
        total = total.saturating_add(occurrences.get());
    }

    total
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_expiry_report::report_unreadable_expiries, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode},
        context_field::ContextField,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use cassiopeia_unreadable_timestamps::unreadable_expiries::UnreadableExpiries;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    #[test]
    fn an_empty_sink_says_nothing_and_counts_nothing() {
        assert_eq!(report_unreadable_expiries(&NoopReporter::new(), UnreadableExpiries::new()), 0);
    }

    #[test]
    fn every_entity_of_one_type_is_named_by_one_diagnostic_with_the_first_as_exemplar() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableExpiries::new();
        unreadable.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:1"), "the day after tomorrow");
        unreadable.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:2"), "sometime next week");
        unreadable.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:3"), "never");

        let total = report_unreadable_expiries(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 3);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 3);
        assert_eq!(reported[0].code, DiagnosticCode::Expander(ExpanderCode::ExpiryUnreadable));
        assert_eq!(reported[0].defining, [ContextField::EntityType(name("Sensor"))]);
        assert!(
            matches!(
                reported[0].incidental.as_slice(),
                [ContextField::Entities { first, additional: 2 }] if first.as_str() == "urn:ngsi-ld:Sensor:1"
            ),
            "{:?}",
            reported[0].incidental
        );
        assert!(reported[0].headline.contains("Sensor"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("urn:ngsi-ld:Sensor:1"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("the day after tomorrow"), "{}", reported[0].headline);
    }

    #[test]
    fn two_types_are_two_diagnostics_in_the_order_they_failed() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableExpiries::new();
        unreadable.record(&name("Station"), &urn("urn:ngsi-ld:Station:1"), "later");
        unreadable.record(&name("Sensor"), &urn("urn:ngsi-ld:Sensor:1"), "earlier");

        let total = report_unreadable_expiries(&sink, unreadable);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].defining, [ContextField::EntityType(name("Station"))]);
        assert_eq!(reported[1].defining, [ContextField::EntityType(name("Sensor"))]);
    }
}

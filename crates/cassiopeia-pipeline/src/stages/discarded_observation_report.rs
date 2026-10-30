use cassiopeia_diagnostic::{
    cause::causes_of,
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::{ContextField, minted_id},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_resolver::discarded_observations::{DiscardedObservation, DiscardedObservations};

/// The one-line summary: which template failed and where it is declared, an entity it failed for,
/// and that the record was discarded for another record of that entity.
///
/// The record that keeps an entity's slot reaches the extractor, which reports its own failure as
/// the attribute it dropped; the records named here never got that far, so the headline says what
/// they cost instead. The entity rides on the headline because the concise form shows nothing else.
fn headline(discarded: &DiscardedObservation) -> String {
    let records = match discarded.records.get() {
        1 => "1 record ranked as carrying no observation time and was".to_string(),
        more => format!("{more} records ranked as carrying no observation time and were"),
    };
    format!(
        "{} for `{}`, so {records} discarded for another record of the same entity",
        discarded.error, discarded.entity
    )
}

/// Names each mapping whose failing `observedAt` template cost records, once, and returns how many
/// records it cost.
///
/// The code is the one the extractor reports the same failure under when the failing record is the
/// one kept, so a template failure reads the same whichever record of an entity won its ranking.
/// Each discarded record is one warning, as each record the extractor reports is.
pub(crate) fn report_discarded_observations(sink: &dyn DiagnosticSink, discarded: DiscardedObservations) -> u64 {
    if discarded.is_empty() {
        return 0;
    }

    discarded.into_entries().into_iter().fold(0, |total: u64, entry| {
        let entities = minted_id(&entry.entity).map(|first| ContextField::Entities {
            first,
            additional: entry.entities.get() - 1,
        });
        sink.report(
            &DiagnosticBuilder::new(
                Severity::Warning,
                DiagnosticCode::Extractor(ExtractorCode::TemplateUnresolvable),
                headline(&entry),
            )
            .with_causes(causes_of(&entry.error))
            .with_optional_context(entities)
            .with_occurrences(entry.records)
            .build(),
        );
        total.saturating_add(entry.records.get())
    })
}

#[cfg(test)]
mod tests {
    use crate::{stages::discarded_observation_report::report_discarded_observations, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode};
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_resolver::{discarded_observations::DiscardedObservations, mapping_id::MappingId, observed_at_failures::ObservedAtFailures};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU64, path::Path, sync::Arc};
    use urn_rs::Urn;

    fn failure() -> TemplateError {
        TemplateError {
            location: TemplateLocation::new(Arc::from(Path::new("thing.json5")), TemplateSite::ObservedAt),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new("{{ ts | trim }}"),
                source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
            }),
        }
    }

    fn discarded(entities: &[(&str, u64)]) -> DiscardedObservations {
        let failures = ObservedAtFailures::new();
        failures.record(MappingId::new(1), failure());
        let mut tally = DiscardedObservations::new();
        for (entity, records) in entities {
            let entity: Urn = entity.parse().unwrap();
            tally.record(MappingId::new(1), &entity, NonZeroU64::new(*records).unwrap(), &failures);
        }
        tally
    }

    #[test]
    fn nothing_discarded_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_discarded_observations(&sink, DiscardedObservations::new()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_mapping_is_named_once_under_the_template_code_and_counts_every_record() {
        let sink = RecordingReporter::new();

        let total = report_discarded_observations(&sink, discarded(&[("urn:ngsi-ld:Thing:A", 2), ("urn:ngsi-ld:Thing:B", 1)]));

        let reported = sink.diagnostics();
        assert_eq!(total, 3);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 3);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::TemplateUnresolvable));
        assert_eq!(reported[0].causes[0], "`{{ ts | trim }}` did not render its value as JSON");
    }

    #[test]
    fn the_headline_names_the_declaration_the_entity_and_the_discarded_records() {
        let sink = RecordingReporter::new();

        report_discarded_observations(&sink, discarded(&[("urn:ngsi-ld:Thing:A", 1)]));

        assert_eq!(
            sink.diagnostics()[0].headline,
            "The observedAt template in the mapping document at 'thing.json5' could not be resolved for `urn:ngsi-ld:Thing:A`, so 1 record ranked as carrying no observation time and was discarded for another record of the same entity"
        );
    }
}

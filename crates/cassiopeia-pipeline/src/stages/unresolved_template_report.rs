use cassiopeia_diagnostic::{
    cause::causes_of,
    code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode},
    context_field::{ContextField, minted_id},
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_extractor::unresolved_template::UnresolvedTemplate;
use cassiopeia_ngsi_ld::entity::name::NameBuf;

/// The one-line summary: which template failed and where it is declared, an entity it failed for,
/// the attribute that was dropped, and that the entity was kept.
///
/// The headline carries the entity because the concise form shows nothing else: the entities travel
/// as a context field, which is only rendered under `-v`, and a reader who cannot see which record
/// failed has nothing to go and inspect. The dropped attribute is named separately because a failing
/// nested property costs the whole attribute it is declared under.
fn headline(attribute: &NameBuf, record: &UnresolvedTemplate) -> String {
    format!(
        "{} for `{}`, so attribute `{attribute}` was dropped; the entity itself was kept",
        record.error, record.entity
    )
}

/// The diagnostic naming one attribute a template failure dropped.
///
/// One diagnostic per attribute, however many entities lost it: the attribute and the code are the
/// failure, the first entity rides on the headline and as an incidental context field, and the
/// first failure's cause chain names the template as written with what to change, then what Tera
/// objected to.
pub(crate) fn unresolved_template_diagnostic(attribute: NameBuf, record: &UnresolvedTemplate) -> Diagnostic {
    let entities = minted_id(&record.entity).map(|first| ContextField::Entities {
        first,
        additional: record.occurrences.get() - 1,
    });

    DiagnosticBuilder::new(
        Severity::Warning,
        DiagnosticCode::Extractor(ExtractorCode::TemplateUnresolvable),
        headline(&attribute, record),
    )
    .with_causes(causes_of(&record.error))
    .with_context(ContextField::Attribute(attribute))
    .with_optional_context(entities)
    .with_occurrences(record.occurrences)
    .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, unresolved_template_report::unresolved_template_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode};
    use cassiopeia_extractor::unresolved_templates::UnresolvedTemplates;
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().unwrap()
    }

    fn failure() -> TemplateError {
        TemplateError {
            location: TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::Attribute(NameBuf::new("codes").unwrap())),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new("{{ codes | split(pat=' ') }}"),
                source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
            }),
        }
    }

    #[test]
    fn one_attribute_is_named_once_with_its_first_entity_however_many_lost_it() {
        let sink = RecordingReporter::new();
        let unresolved = UnresolvedTemplates::new();
        unresolved.record(&name("dafifCode"), &urn("urn:ngsi-ld:Country:Bonaire"), failure());
        unresolved.record(&name("dafifCode"), &urn("urn:ngsi-ld:Country:Curacao"), failure());

        let total = report_sink(&sink, unresolved, unresolved_template_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::TemplateUnresolvable));
        assert!(reported[0].headline.contains("dafifCode"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("urn:ngsi-ld:Country:Bonaire"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("kept"), "{}", reported[0].headline);
    }

    #[test]
    fn the_headline_names_the_declaration_the_entity_and_the_dropped_attribute_and_the_causes_carry_the_hint() {
        let sink = RecordingReporter::new();
        let unresolved = UnresolvedTemplates::new();
        unresolved.record(&name("codes"), &urn("urn:ngsi-ld:Sensor:a"), failure());

        report_sink(&sink, unresolved, unresolved_template_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(
            reported[0].headline,
            "The attribute `codes` template in the mapping document at 'sensor.json5' could not be resolved for `urn:ngsi-ld:Sensor:a`, so attribute `codes` was dropped; the entity itself was kept"
        );
        assert_eq!(reported[0].causes[0], "`{{ codes | split(pat=' ') }}` did not render its value as JSON");
    }
}

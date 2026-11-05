use cassiopeia_diagnostic::{
    cause::causes_of,
    code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode},
    context_field::{ContextField, minted_id},
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ir::relationship_path::RelationshipPath;
use cassiopeia_ngsi_ld::entity::attribute_path::AttributePath;
use cassiopeia_unresolved_templates::unresolved_template::UnresolvedTemplate;

/// The one-line summary: which template failed and where it is declared, an entity it failed for,
/// the relationship that was left out, and that the entity was kept.
///
/// The headline carries the entity because the concise form shows nothing else: the entities travel
/// as a context field, which is only rendered under `-v`, and a reader who cannot see which record
/// failed has nothing to go and inspect. The relationship is named by its path from the entity,
/// since a nested relationship costs only itself, not the attribute it is declared under.
fn headline(relationship: &RelationshipPath, record: &UnresolvedTemplate) -> String {
    format!(
        "{} for `{}`, so relationship `{relationship}` was left out; the entity itself was kept",
        record.error, record.entity
    )
}

/// The diagnostic naming one relationship a template failure left out.
///
/// One diagnostic per relationship, however many entities lost it: the relationship and the code
/// are the failure, the first entity rides on the headline and as an incidental context field, and
/// the first failure's cause chain names the template as written with what to change, then what
/// Tera objected to. The code is the expander's, the stage that mints relationship objects.
pub(crate) fn unresolved_link_diagnostic(relationship: RelationshipPath, record: &UnresolvedTemplate) -> Diagnostic {
    let entities = minted_id(&record.entity).map(|first| ContextField::Entities {
        first,
        additional: record.occurrences.get() - 1,
    });
    let headline = headline(&relationship, record);
    // The top-level attribute is the context, as for every other attribute-level diagnostic; a path
    // always has one, since every path is built from the attribute it starts at.
    let attribute = match relationship {
        RelationshipPath::Flat(attribute) => Some(attribute),
        RelationshipPath::Nested(segments) => segments.into_vec().into_iter().next(),
    }
    .map(|root| ContextField::Attribute(AttributePath::top_level(root)));

    DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Expander(ExpanderCode::RelationshipUnresolvable), headline)
        .with_causes(causes_of(&record.error))
        .with_optional_context(attribute)
        .with_optional_context(entities)
        .with_occurrences(record.occurrences)
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, unresolved_link_report::unresolved_link_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode},
        context_field::ContextField,
    };
    use cassiopeia_ir::relationship_path::RelationshipPath;
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use cassiopeia_unresolved_templates::unresolved_templates::UnresolvedTemplates;
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
            location: TemplateLocation::new(Arc::from(Path::new("thing.json5")), TemplateSite::Attribute(name("refRoad"))),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new("{{ road | split(pat=' ') }}"),
                source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
            }),
        }
    }

    #[test]
    fn one_relationship_is_named_once_with_its_first_entity_however_many_lost_it() {
        let sink = RecordingReporter::new();
        let unresolved = UnresolvedTemplates::new();
        unresolved.record(&RelationshipPath::flat(name("refRoad")), &urn("urn:ngsi-ld:Thing:B"), failure());
        unresolved.record(&RelationshipPath::flat(name("refRoad")), &urn("urn:ngsi-ld:Thing:C"), failure());

        let total = report_sink(&sink, unresolved, unresolved_link_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[0].code, DiagnosticCode::Expander(ExpanderCode::RelationshipUnresolvable));
        assert_eq!(
            reported[0].headline,
            "The attribute `refRoad` template in the mapping document at 'thing.json5' could not be resolved for `urn:ngsi-ld:Thing:B`, so relationship `refRoad` was left out; the entity itself was kept"
        );
        assert_eq!(reported[0].causes[0], "`{{ road | split(pat=' ') }}` did not render its value as JSON");
        assert_eq!(reported[0].defining, [ContextField::Attribute(AttributePath::top_level(name("refRoad")))]);
    }

    #[test]
    fn a_nested_relationship_is_named_by_its_path_and_its_top_level_attribute_is_the_context() {
        let sink = RecordingReporter::new();
        let unresolved = UnresolvedTemplates::new();
        unresolved.record(
            &RelationshipPath::from_segments(vec![name("hasLeadActor"), name("playsCharacter")]),
            &urn("urn:ngsi-ld:Movie:M-1"),
            failure(),
        );

        report_sink(&sink, unresolved, unresolved_link_diagnostic);

        let reported = sink.diagnostics();
        assert!(
            reported[0]
                .headline
                .ends_with("so relationship `hasLeadActor.playsCharacter` was left out; the entity itself was kept"),
            "{}",
            reported[0].headline
        );
        assert_eq!(reported[0].defining, [ContextField::Attribute(AttributePath::top_level(name("hasLeadActor")))]);
    }
}

use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, mapping_code::MappingCode},
    context_field::ContextField,
    diagnostic::Diagnostic,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_mapping::{
    ignored_key::{IgnoredKey, IgnoredKeyKind, IneffectiveKey},
    mapping::Mapping,
    version::Version,
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::path::Path;

/// The code an ignored key of a mapping of `version` is counted under: what a key means, and
/// whether it means anything, is fixed by the mapping format.
const fn code(version: Version, kind: &IgnoredKeyKind) -> DiagnosticCode {
    match (version, kind) {
        (Version::V4, IgnoredKeyKind::Unknown(_)) => DiagnosticCode::Mapping(MappingCode::V4UnknownKey),
        (Version::V4, IgnoredKeyKind::WithoutEffect(_)) => DiagnosticCode::Mapping(MappingCode::V4KeyWithoutEffect),
    }
}

/// The one-line summary: the document's top-level attribute the key is found beneath, and the
/// document, which is all an author needs to find the declaration.
fn headline(document: &Path, attribute: &NameBuf) -> String {
    format!(
        "Attribute `{attribute}` in the mapping document at '{}' declares a key that has no effect",
        document.display()
    )
}

/// The declaration holding the key, the key, and what to do about it.
///
/// The cause rides on the concise form, so it names the declaration exactly, down to an instance's
/// position or a `languageMap` entry's language tag.
fn cause(ignored: &IgnoredKey) -> String {
    let site = ignored.site();
    match ignored.kind() {
        IgnoredKeyKind::Unknown(key) => format!("{site} declares `{key}`, which it does not read: correct the key or remove it"),
        IgnoredKeyKind::WithoutEffect(key @ (IneffectiveKey::SourceField | IneffectiveKey::IdPattern)) => {
            format!("{site} declares `{key}`, which nothing reads, since a relationship's object is the entity its `source` names: remove the key")
        }
        IgnoredKeyKind::WithoutEffect(IneffectiveKey::SyntheticEntity) => format!(
            "{site} declares a `syntheticEntity`, which is written only from a top-level Relationship or ListRelationship of the mapping, so no entity is written for it: declare it there or remove it"
        ),
    }
}

/// The warning naming one key the mapping document at `document`, of `version`, declares without
/// effect.
///
/// The document and the top-level attribute are its defining context, and the cause names the key,
/// so two keys, two attributes declaring the same key, and two documents declaring the same
/// attribute stay distinct findings.
fn ignored_key_diagnostic(document: &Path, version: Version, ignored: &IgnoredKey) -> Diagnostic {
    let attribute = ignored.site().top_level_attribute();
    // The diagnostic outlives the mapping that names the document and the attribute.
    DiagnosticBuilder::new(Severity::Warning, code(version, ignored.kind()), headline(document, attribute))
        .with_cause(cause(ignored))
        .with_context(ContextField::MappingDocument(document.to_path_buf()))
        .with_context(ContextField::Attribute(AttributePath::top_level(attribute.clone())))
        .build()
}

/// Names every key `mappings` declare without effect, each mapping with the path it was loaded from,
/// once each, and returns how many keys that was.
///
/// One warning per key: each key is its own mistake, declared in one place. A run reports these
/// once, before any record is read, so a mapping that several inputs share is named once.
pub(crate) fn report_ignored_keys<'a>(sink: &dyn DiagnosticSink, mappings: impl IntoIterator<Item = (&'a Path, &'a Mapping)>) -> u64 {
    mappings
        .into_iter()
        .flat_map(|(document, mapping)| {
            mapping
                .ignored_keys()
                .iter()
                .map(move |ignored| ignored_key_diagnostic(document, *mapping.version(), ignored))
        })
        .fold(0, |total: u64, diagnostic| {
            sink.report(&diagnostic);
            total.saturating_add(1)
        })
}

#[cfg(test)]
mod tests {
    use crate::{ignored_key_report::report_ignored_keys, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, mapping_code::MappingCode},
        context_field::ContextField,
    };
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::{assert_eq, assert_ne};
    use std::path::{Path, PathBuf};

    /// The sensors mapping of the issue that brought these warnings: a misspelled transformation, a
    /// target's `sourceField` and `idPattern`, and a synthetic entity on a Property.
    const SENSORS: &str = r#"{
        version: "v4",
        dataModel: "Device",
        identity: { entityName: "{{ sensor }}" },
        attributes: {
            temperature: { source: "{{ t }}", transformaton: "float" },
            refBuilding: {
                type: "Relationship",
                source: "{{ building }}",
                target: { entity: "Building", sourceField: "building_uri", idPattern: "B-\\d+" },
            },
            manufacturer: {
                source: "{{ maker }}",
                syntheticEntity: {
                    dataModel: "Organization",
                    identity: { entityName: "{{ maker }}" },
                    attributes: { name: { source: "{{ maker }}" } },
                },
            },
        },
    }"#;

    fn loaded(document: &str, origin: &str) -> Mapping {
        Mapping::from_json5(document, Path::new(origin), &mut TemplateRunner::new()).expect("the mapping loads")
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_mapping_without_ignored_keys_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();
        let mapping = loaded(
            r#"{ version: "v4", dataModel: "Device", identity: { entityName: "{{ id }}" }, attributes: { name: { source: "{{ name }}" } } }"#,
            "device.json5",
        );

        assert_eq!(report_ignored_keys(&sink, [(Path::new("device.json5"), &mapping)]), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn each_ignored_key_is_one_warning_under_its_code_naming_the_attribute_the_document_and_the_declaration() {
        let sink = RecordingReporter::new();
        let mapping = loaded(SENSORS, "/maps/sensors.json5");

        let total = report_ignored_keys(&sink, [(Path::new("/maps/sensors.json5"), &mapping)]);

        let reported = sink.diagnostics();
        assert_eq!(total, 4);
        assert_eq!(
            reported
                .iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.headline.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::Mapping(MappingCode::V4UnknownKey),
                    "Attribute `temperature` in the mapping document at '/maps/sensors.json5' declares a key that has no effect"
                ),
                (
                    DiagnosticCode::Mapping(MappingCode::V4KeyWithoutEffect),
                    "Attribute `refBuilding` in the mapping document at '/maps/sensors.json5' declares a key that has no effect"
                ),
                (
                    DiagnosticCode::Mapping(MappingCode::V4KeyWithoutEffect),
                    "Attribute `refBuilding` in the mapping document at '/maps/sensors.json5' declares a key that has no effect"
                ),
                (
                    DiagnosticCode::Mapping(MappingCode::V4KeyWithoutEffect),
                    "Attribute `manufacturer` in the mapping document at '/maps/sensors.json5' declares a key that has no effect"
                ),
            ]
        );
        assert_eq!(
            reported.iter().map(|diagnostic| diagnostic.causes.clone()).collect::<Vec<_>>(),
            [
                vec!["`temperature` declares `transformaton`, which it does not read: correct the key or remove it".to_owned()],
                vec![
                    "The target of `refBuilding` declares `sourceField`, which nothing reads, since a relationship's object is the entity its `source` names: remove the key".to_owned()
                ],
                vec![
                    "The target of `refBuilding` declares `idPattern`, which nothing reads, since a relationship's object is the entity its `source` names: remove the key".to_owned()
                ],
                vec![
                    "`manufacturer` declares a `syntheticEntity`, which is written only from a top-level Relationship or ListRelationship of the mapping, so no entity is written for it: declare it there or remove it".to_owned()
                ],
            ]
        );
        assert!(reported.iter().all(|diagnostic| diagnostic.occurrences == 1));
        assert_eq!(
            reported[0].defining,
            [
                ContextField::Attribute(AttributePath::top_level(name("temperature"))),
                ContextField::MappingDocument(PathBuf::from("/maps/sensors.json5")),
            ]
        );
    }

    #[test]
    fn a_key_inside_a_synthetic_entity_is_reported_under_the_attribute_declaring_the_synthetic_entity() {
        let sink = RecordingReporter::new();
        let mapping = loaded(
            r#"{
                version: "v4",
                dataModel: "Station",
                identity: { entityName: "{{ station }}" },
                attributes: {
                    owner: {
                        type: "Relationship",
                        syntheticEntity: {
                            dataModel: "Organization",
                            identity: { entityName: "{{ owner_id }}" },
                            attributes: { name: { source: "{{ owner_name }}", trasformation: "string" } },
                        },
                    },
                },
            }"#,
            "stations.json5",
        );

        report_ignored_keys(&sink, [(Path::new("stations.json5"), &mapping)]);

        let reported = sink.diagnostics();
        assert_eq!(
            reported[0].headline,
            "Attribute `owner` in the mapping document at 'stations.json5' declares a key that has no effect"
        );
        assert_eq!(
            reported[0].causes,
            ["`name` in the syntheticEntity of `owner` declares `trasformation`, which it does not read: correct the key or remove it"]
        );
        assert_eq!(reported[0].defining[0], ContextField::Attribute(AttributePath::top_level(name("owner"))));
    }

    #[test]
    fn two_documents_declaring_the_same_key_are_two_findings() {
        let sink = RecordingReporter::new();
        let document = r#"{ version: "v4", dataModel: "Device", identity: { entityName: "{{ id }}" }, attributes: { name: { source: "{{ name }}", sorce: "{{ id }}" } } }"#;
        let (first, second) = (loaded(document, "a.json5"), loaded(document, "b.json5"));

        let total = report_ignored_keys(&sink, [(Path::new("a.json5"), &first), (Path::new("b.json5"), &second)]);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 2);
        assert_ne!(reported[0].defining, reported[1].defining);
    }
}

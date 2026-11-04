use crate::output::validation_mode::ValidationMode;
use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource};
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use typed_builder::TypedBuilder;

/// Everything a run decides about validation, grouped under `output.validation`.
///
/// Each knob is optional: an absent field falls back to the run default, or to a CLI override when
/// one applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestValidation {
    /// How strictly schema validation is enforced before an entity is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    mode: Option<ValidationMode>,

    /// A custom JSON Schema applied to every entity type no per-input `schema` already covers. Local
    /// file or remote URL; absent leaves the Smart Data Models convention in force.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    schema: Option<SchemaSource>,

    /// The NGSI-LD representation entities are checked in. Absent validates the simplified form, the
    /// shape a Smart Data Model schema describes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    representation: Option<NgsiLdRepresentation>,

    /// Where a JSON validation report is written. Absent writes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    report: Option<PathBuf>,
}

impl ManifestValidation {
    /// Resolves a relative local `schema` and a relative `report` path against `directory`. Absolute
    /// paths and a remote schema are kept as they are.
    #[must_use]
    pub fn resolved_against(self, directory: &Path) -> ManifestValidation {
        ManifestValidation {
            schema: self.schema.map(|schema| schema.resolved_against(directory)),
            report: self.report.map(|report| directory.join(report)),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::output::{validation::ManifestValidation, validation_mode::ValidationMode};
    use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource};
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    #[test]
    fn reads_a_full_validation_object() {
        let validation: ManifestValidation =
            serde_json::from_str(r#"{"mode": "fail", "schema": "x.schema.json", "representation": "normalized", "report": "report.json"}"#).unwrap();

        assert_eq!(validation.mode(), &Some(ValidationMode::Fail));
        assert_eq!(validation.schema(), &Some(SchemaSource::Local(PathBuf::from("x.schema.json"))));
        assert_eq!(validation.representation(), &Some(NgsiLdRepresentation::Normalized));
        assert_eq!(validation.report(), &Some(PathBuf::from("report.json")));
    }

    #[test]
    fn an_empty_object_leaves_every_knob_unset() {
        let validation: ManifestValidation = serde_json::from_str("{}").unwrap();

        assert_eq!(validation.mode(), &None);
        assert_eq!(validation.schema(), &None);
        assert_eq!(validation.representation(), &None);
        assert_eq!(validation.report(), &None);
    }

    #[test]
    fn a_validation_object_resolves_its_relative_schema_and_report_against_the_given_directory() {
        let validation: ManifestValidation = serde_json::from_str(r#"{"mode": "fail", "schema": "x.schema.json", "report": "report.json"}"#).unwrap();
        let validation = validation.resolved_against(Path::new("runs/city"));

        assert_eq!(validation.mode(), &Some(ValidationMode::Fail));
        assert_eq!(validation.schema(), &Some(SchemaSource::Local(PathBuf::from("runs/city/x.schema.json"))));
        assert_eq!(validation.report(), &Some(PathBuf::from("runs/city/report.json")));
    }

    #[test]
    fn a_validation_object_keeps_an_absolute_report_and_a_remote_schema_when_resolved() {
        let validation: ManifestValidation = serde_json::from_str(r#"{"schema": "https://example.org/x.json", "report": "/srv/report.json"}"#).unwrap();
        let validation = validation.resolved_against(Path::new("runs/city"));

        assert_eq!(
            validation.schema(),
            &Some(SchemaSource::Remote(url::Url::parse("https://example.org/x.json").unwrap()))
        );
        assert_eq!(validation.report(), &Some(PathBuf::from("/srv/report.json")));
    }
}

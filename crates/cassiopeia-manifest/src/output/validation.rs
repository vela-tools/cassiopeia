use crate::output::validation_mode::ValidationMode;
use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource};
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

#[cfg(test)]
mod tests {
    use crate::output::{validation::ManifestValidation, validation_mode::ValidationMode};
    use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

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
}

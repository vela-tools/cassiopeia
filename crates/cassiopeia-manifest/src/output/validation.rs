use crate::output::validation_mode::ValidationMode;
use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource, skip_null::NgsiLdSkipNull};
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use typed_builder::TypedBuilder;

/// Everything a run decides about validation, grouped under `output.validation`.
///
/// Each knob is optional: an absent field falls back to the run default, or to a CLI override when
/// one applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
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

    /// Whether null-valued attributes take part in the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    skip_null: Option<NgsiLdSkipNull>,

    /// Where a JSON validation report is written. Absent writes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    report: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use crate::output::{validation::ManifestValidation, validation_mode::ValidationMode};
    use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource, skip_null::NgsiLdSkipNull};
    use std::path::PathBuf;

    #[test]
    fn reads_a_full_validation_object() {
        let validation: ManifestValidation = serde_json::from_str(
            r#"{"mode": "fail", "schema": "x.schema.json", "representation": "normalized", "skipNull": "include", "report": "report.json"}"#,
        )
        .unwrap();

        assert_eq!(validation.mode(), &Some(ValidationMode::Fail));
        assert_eq!(validation.schema(), &Some(SchemaSource::Local(PathBuf::from("x.schema.json"))));
        assert_eq!(validation.representation(), &Some(NgsiLdRepresentation::Normalized));
        assert_eq!(validation.skip_null(), &Some(NgsiLdSkipNull::Include));
        assert_eq!(validation.report(), &Some(PathBuf::from("report.json")));
    }

    #[test]
    fn an_empty_object_leaves_every_knob_unset() {
        let validation: ManifestValidation = serde_json::from_str("{}").unwrap();

        assert_eq!(validation.mode(), &None);
        assert_eq!(validation.schema(), &None);
        assert_eq!(validation.representation(), &None);
        assert_eq!(validation.skip_null(), &None);
        assert_eq!(validation.report(), &None);
    }

    #[test]
    fn skip_null_uses_its_camel_case_key_and_round_trips() {
        let validation = ManifestValidation::builder().skip_null(Some(NgsiLdSkipNull::Skip)).build();

        let encoded = serde_json::to_string(&validation).unwrap();
        assert!(encoded.contains("skipNull"));
        assert_eq!(serde_json::from_str::<ManifestValidation>(&encoded).unwrap(), validation);
    }
}

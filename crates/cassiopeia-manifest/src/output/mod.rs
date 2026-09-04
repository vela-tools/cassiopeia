pub mod context_delivery;
pub mod destination;
pub mod temporal;
pub mod user_agent;
pub mod validation;
pub mod validation_mode;

use crate::output::{destination::Destination, temporal::ManifestTemporal, validation::ManifestValidation};
use cassiopeia_common::{context::mode::AtContextMode, representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use getset::Getters;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// Where a run's entities go and in what shape they are written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ManifestOutput {
    /// The destination and the settings only that destination understands.
    #[serde(flatten)]
    #[getset(get = "pub")]
    destination: Destination,

    /// The NGSI-LD representation entities are written in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    representation: Option<NgsiLdRepresentation>,

    /// Whether null-valued attributes are written or omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    skip_null: Option<NgsiLdSkipNull>,

    /// The `@context` mode applied to every entity that does not override it per input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    context: Option<AtContextMode>,

    /// How this run validates: mode, custom schema, representation, skip-null, and report path,
    /// grouped under one `validation` object rather than scattered as flat siblings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    validation: Option<ManifestValidation>,

    /// The temporal output shape (ETSI GS CIM 009 v1.9.1 clause 5.2.20). Absent means current-state:
    /// each id written once, carrying the latest observation of every attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    temporal: Option<ManifestTemporal>,
}

#[cfg(test)]
mod tests {
    use crate::output::{
        ManifestOutput,
        destination::Destination,
        temporal::{ManifestTemporal, TemporalRepresentation},
        validation_mode::ValidationMode,
    };
    use cassiopeia_common::{file_framing::FileFraming, representation::NgsiLdRepresentation, schema_source::SchemaSource};
    use std::path::PathBuf;

    #[test]
    fn reads_a_full_nested_validation_object() {
        let output: ManifestOutput = serde_json::from_str(
            r#"{"target": "file", "directory": "out", "validation": {"mode": "fail", "schema": "x.schema.json", "representation": "normalized", "skipNull": "skip", "report": "report.json"}}"#,
        )
        .unwrap();

        let validation = output.validation().as_ref().unwrap();
        assert_eq!(validation.mode(), &Some(ValidationMode::Fail));
        assert_eq!(validation.schema(), &Some(SchemaSource::Local(PathBuf::from("x.schema.json"))));
        assert_eq!(validation.representation(), &Some(NgsiLdRepresentation::Normalized));
    }

    #[test]
    fn a_nested_validation_object_round_trips() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out", "validation": {"mode": "fail"}}"#).unwrap();

        let encoded = serde_json::to_string(&output).unwrap();
        assert_eq!(serde_json::from_str::<ManifestOutput>(&encoded).unwrap(), output);
    }

    #[test]
    fn reads_a_file_output_with_a_representation() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out", "representation": "concise"}"#).unwrap();

        assert_eq!(
            output.destination(),
            &Destination::File {
                directory: Some(PathBuf::from("out")),
                framing: FileFraming::Array
            }
        );
        assert_eq!(output.representation(), &Some(NgsiLdRepresentation::Concise));
    }

    #[test]
    fn a_file_output_round_trips_through_json() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out"}"#).unwrap();
        let encoded = serde_json::to_string(&output).unwrap();

        assert_eq!(serde_json::from_str::<ManifestOutput>(&encoded).unwrap(), output);
    }

    #[test]
    fn an_output_without_a_target_is_rejected() {
        assert!(serde_json::from_str::<ManifestOutput>(r#"{"directory": "out"}"#).is_err());
    }

    #[test]
    fn an_output_without_a_validation_object_is_none_and_round_trips() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out"}"#).unwrap();

        assert_eq!(output.validation(), &None);
        let encoded = serde_json::to_string(&output).unwrap();
        assert!(!encoded.contains("validation"));
        assert_eq!(serde_json::from_str::<ManifestOutput>(&encoded).unwrap(), output);
    }

    #[test]
    fn reads_a_broker_output_with_a_series_temporal_representation() {
        let output: ManifestOutput =
            serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:9090/", "temporal": {"representation": "series"}}"#).unwrap();

        assert_eq!(
            output.temporal().as_ref().map(ManifestTemporal::representation),
            Some(&TemporalRepresentation::Series)
        );
    }

    #[test]
    fn a_temporal_series_object_round_trips() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out", "temporal": {"representation": "series"}}"#).unwrap();

        let encoded = serde_json::to_string(&output).unwrap();
        assert_eq!(serde_json::from_str::<ManifestOutput>(&encoded).unwrap(), output);
    }

    #[test]
    fn an_output_without_a_temporal_object_is_none_and_omits_the_key() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out"}"#).unwrap();

        assert_eq!(output.temporal(), &None);
        let encoded = serde_json::to_string(&output).unwrap();
        assert!(!encoded.contains("temporal"));
        assert_eq!(serde_json::from_str::<ManifestOutput>(&encoded).unwrap(), output);
    }
}

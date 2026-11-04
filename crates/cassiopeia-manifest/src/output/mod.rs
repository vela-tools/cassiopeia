pub mod context_delivery;
pub mod destination;
pub mod temporal;
pub mod validation;
pub mod validation_mode;

use crate::{
    error::ManifestError,
    output::{destination::Destination, temporal::ManifestTemporal, validation::ManifestValidation},
};
use cassiopeia_common::{context::mode::AtContextMode, representation::NgsiLdRepresentation};
use getset::Getters;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use std::path::Path;
use typed_builder::TypedBuilder;

/// Where a run's entities go and in what shape they are written.
///
/// It is read through [`ManifestOutputRaw`], because serde's `flatten`, which places the destination's
/// settings beside the output's own, cannot be combined with `deny_unknown_fields`: the raw struct
/// hands every key the output does not define to the destination, which refuses any it does not
/// define either, so a misspelt or retired key is an error rather than silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase", try_from = "ManifestOutputRaw")]
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

    /// The `@context` mode applied to every entity that does not override it per input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    context: Option<AtContextMode>,

    /// How this run validates: mode, custom schema, representation, and report path, grouped
    /// under one `validation` object rather than scattered as flat siblings.
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

impl ManifestOutput {
    /// Resolves the output's relative local paths against `directory`: a file destination's
    /// `directory`, a local `context` file, and the validation `schema` and `report`. Absolute paths
    /// and URLs are kept as they are.
    #[must_use]
    pub fn resolved_against(self, directory: &Path) -> ManifestOutput {
        ManifestOutput {
            destination: self.destination.resolved_against(directory),
            context: self.context.map(|context| context.resolved_against(directory)),
            validation: self.validation.map(|validation| validation.resolved_against(directory)),
            ..self
        }
    }
}

/// The on-the-wire shape of a manifest output, before its destination settings are read.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestOutputRaw {
    #[serde(default)]
    representation: Option<NgsiLdRepresentation>,
    #[serde(default)]
    context: Option<AtContextMode>,
    #[serde(default)]
    validation: Option<ManifestValidation>,
    #[serde(default)]
    temporal: Option<ManifestTemporal>,
    /// The `target` and every other key the output does not define, read as the destination.
    #[serde(flatten)]
    destination: Map<String, JsonValue>,
}

impl TryFrom<ManifestOutputRaw> for ManifestOutput {
    type Error = ManifestError;

    fn try_from(raw: ManifestOutputRaw) -> Result<ManifestOutput, ManifestError> {
        let destination = Destination::deserialize(JsonValue::Object(raw.destination)).map_err(|source| ManifestError::Destination { source })?;

        Ok(ManifestOutput {
            destination,
            representation: raw.representation,
            context: raw.context,
            validation: raw.validation,
            temporal: raw.temporal,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::output::{
        ManifestOutput,
        destination::Destination,
        temporal::{ManifestTemporal, TemporalRepresentation},
        validation_mode::ValidationMode,
    };
    use cassiopeia_common::{context::mode::AtContextMode, file_framing::FileFraming, representation::NgsiLdRepresentation, schema_source::SchemaSource};
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    #[test]
    fn reads_a_full_nested_validation_object() {
        let output: ManifestOutput = serde_json::from_str(
            r#"{"target": "file", "directory": "out", "validation": {"mode": "fail", "schema": "x.schema.json", "representation": "normalized", "report": "report.json"}}"#,
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
    fn an_output_carrying_the_retired_skip_null_key_is_rejected_as_an_unknown_field() {
        let error = serde_json::from_str::<ManifestOutput>(r#"{"target": "file", "directory": "out", "skipNull": "include"}"#).unwrap_err();

        assert!(error.to_string().contains("unknown field `skipNull`"), "{error}");
    }

    #[test]
    fn a_broker_output_carrying_the_retired_skip_null_key_is_rejected_as_an_unknown_field() {
        let error = serde_json::from_str::<ManifestOutput>(r#"{"target": "context-broker", "url": "http://localhost:9090/", "skipNull": "skip"}"#).unwrap_err();

        assert!(error.to_string().contains("unknown field `skipNull`"), "{error}");
    }

    #[test]
    fn a_validation_object_carrying_the_retired_skip_null_key_is_rejected_as_an_unknown_field() {
        let error = serde_json::from_str::<ManifestOutput>(r#"{"target": "file", "directory": "out", "validation": {"mode": "fail", "skipNull": "include"}}"#)
            .unwrap_err();

        assert!(error.to_string().contains("unknown field `skipNull`"), "{error}");
    }

    #[test]
    fn a_broker_output_round_trips_through_json() {
        let output: ManifestOutput =
            serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:9090/", "tenant": "city", "representation": "concise"}"#).unwrap();
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

    #[test]
    fn a_file_output_resolves_its_directory_context_and_validation_paths_against_the_given_directory() {
        let output: ManifestOutput = serde_json::from_str(
            r#"{"target": "file", "directory": "out", "context": "ctx.jsonld", "validation": {"schema": "x.schema.json", "report": "report.json"}}"#,
        )
        .unwrap();
        let output = output.resolved_against(Path::new("runs/city"));

        assert_eq!(
            output.destination(),
            &Destination::File {
                directory: Some(PathBuf::from("runs/city/out")),
                framing: FileFraming::Array
            }
        );
        assert_eq!(output.context(), &Some(AtContextMode::Local(PathBuf::from("runs/city/ctx.jsonld"))));
        let validation = output.validation().as_ref().unwrap();
        assert_eq!(validation.schema(), &Some(SchemaSource::Local(PathBuf::from("runs/city/x.schema.json"))));
        assert_eq!(validation.report(), &Some(PathBuf::from("runs/city/report.json")));
    }

    #[test]
    fn a_file_output_without_a_directory_keeps_the_run_time_default_when_resolved() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file"}"#).unwrap();
        let output = output.resolved_against(Path::new("runs/city"));

        assert_eq!(output.destination(), &Destination::file(None, FileFraming::Array));
    }

    #[test]
    fn a_broker_output_is_unchanged_by_resolution() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:9090/", "tenant": "city"}"#).unwrap();

        assert_eq!(output.clone().resolved_against(Path::new("runs/city")), output);
    }
}

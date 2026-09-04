use crate::{
    error::ManifestError,
    mapping_binding::{CollectionMapping, MappingBinding},
};
use cassiopeia_common::{context::mode::AtContextMode, format::DataFormat, input::Input, schema_source::SchemaSource};
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};
use typed_builder::TypedBuilder;

/// One source a run reads, together with how its records are routed to mappings.
///
/// Validation runs through [`TryFrom<ManifestInputRaw>`], the same `try_from` idiom
/// [`Inputs`](crate::inputs::Inputs) uses, because serde's `flatten` is incompatible with
/// `deny_unknown_fields`: the raw struct keeps the strict field check while the validated struct
/// holds the resolved [`MappingBinding`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(try_from = "ManifestInputRaw", into = "ManifestInputRaw")]
pub struct ManifestInput {
    /// The file or URL the records are read from.
    #[getset(get = "pub")]
    source: Input,

    /// How this source's records are routed to mappings.
    #[getset(get = "pub")]
    mapping_binding: MappingBinding,

    /// The source format, detected from the source itself when it is not stated.
    #[builder(default = None)]
    #[getset(get = "pub")]
    format: Option<DataFormat>,

    /// An `@context` mode for this source alone, overriding the output-level mode.
    #[builder(default = None)]
    #[getset(get = "pub")]
    context: Option<AtContextMode>,

    /// A custom validation schema for the types this source produces, overriding the output-level
    /// global schema for those types. Local file or remote URL; absent leaves the global schema (or
    /// the Smart Data Models convention) in force. Mirrors `context`: the schema override is never
    /// declared on the mapping, only here or on the output.
    #[builder(default = None)]
    #[getset(get = "pub")]
    schema: Option<SchemaSource>,

    /// Run-level variables visible to this source's records alone, overriding a manifest-global
    /// [`vars`](crate::manifest::Manifest::vars) of the same name. Mirrors `context`/`schema`: a
    /// per-input override declared here, never on the mapping. Read in a mapping as `{{ vars.<name> }}`.
    #[builder(default = None)]
    #[getset(get = "pub")]
    vars: Option<serde_json::Map<String, serde_json::Value>>,
}

/// The on-the-wire shape of a manifest input, before its mapping binding is validated.
///
/// `mapping` and `mappings` are the two mutually exclusive ways to bind mappings; exactly one must
/// be present. Keeping them as sibling optional fields lets `deny_unknown_fields` stay in force,
/// which a `flatten`ed enum would forbid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestInputRaw {
    source: Input,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mapping: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mappings: Option<Vec<CollectionMapping>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format: Option<DataFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<AtContextMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<SchemaSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vars: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TryFrom<ManifestInputRaw> for ManifestInput {
    type Error = ManifestError;

    fn try_from(raw: ManifestInputRaw) -> Result<ManifestInput, ManifestError> {
        let mapping_binding = build_binding(raw.mapping, raw.mappings, raw.format)?;

        Ok(ManifestInput {
            source: raw.source,
            mapping_binding,
            format: raw.format,
            context: raw.context,
            schema: raw.schema,
            vars: raw.vars,
        })
    }
}

impl From<ManifestInput> for ManifestInputRaw {
    fn from(input: ManifestInput) -> ManifestInputRaw {
        let (mapping, mappings) = match input.mapping_binding {
            MappingBinding::Single { mapping } => (Some(mapping), None),
            MappingBinding::Collections { mappings } => (None, Some(mappings)),
        };

        ManifestInputRaw {
            source: input.source,
            mapping,
            mappings,
            format: input.format,
            context: input.context,
            schema: input.schema,
            vars: input.vars,
        }
    }
}

/// Validates the two mapping fields into a single [`MappingBinding`].
///
/// The format check is skipped when the format is `Auto`/omitted: the runtime router in the expander
/// is the backstop for a collections binding whose format is only known once the source is read.
fn build_binding(mapping: Option<PathBuf>, mappings: Option<Vec<CollectionMapping>>, format: Option<DataFormat>) -> Result<MappingBinding, ManifestError> {
    match (mapping, mappings) {
        (Some(_), Some(_)) => Err(ManifestError::ConflictingMappingBinding),
        (None, None) => Err(ManifestError::MissingMappingBinding),
        (Some(mapping), None) => Ok(MappingBinding::Single { mapping }),
        (None, Some(mappings)) => {
            if mappings.is_empty() {
                return Err(ManifestError::EmptyCollections);
            }

            let mut seen = HashSet::with_capacity(mappings.len());
            for entry in &mappings {
                if !seen.insert(entry.collection()) {
                    return Err(ManifestError::DuplicateCollection(entry.collection().clone()));
                }
            }

            if let Some(format) = format.and_then(DataFormat::to_option)
                && !format.supports_collections()
            {
                return Err(ManifestError::CollectionsUnsupportedByFormat(format));
            }

            Ok(MappingBinding::Collections { mappings })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ManifestError,
        input::{ManifestInput, ManifestInputRaw},
        mapping_binding::{CollectionMapping, MappingBinding},
    };
    use cassiopeia_common::{collection::CollectionName, context::mode::AtContextMode, format::DataFormat, input::Input, schema_source::SchemaSource};
    use std::path::PathBuf;

    fn parse(json: &str) -> Result<ManifestInput, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn a_lone_mapping_becomes_a_single_binding() {
        let input = parse(r#"{"source": "data/stations.csv", "mapping": "mappings/station.json5"}"#).unwrap();

        assert_eq!(input.source(), &Input::Local(PathBuf::from("data/stations.csv")));
        assert_eq!(
            input.mapping_binding(),
            &MappingBinding::Single {
                mapping: PathBuf::from("mappings/station.json5")
            }
        );
        assert_eq!(input.format(), &None);
    }

    #[test]
    fn a_mappings_list_becomes_a_collections_binding() {
        let input = parse(r#"{"source": "data/sensors.kml", "mappings": [{"collection": "A", "mapping": "a.json5"}]}"#).unwrap();

        assert_eq!(
            input.mapping_binding(),
            &MappingBinding::Collections {
                mappings: vec![CollectionMapping::new(CollectionName::from("A"), PathBuf::from("a.json5"))],
            }
        );
    }

    #[test]
    fn reads_a_format_and_a_context_override() {
        let input = parse(r#"{"source": "data/a.csv", "mapping": "m.json5", "format": "csv", "context": "none", "schema": "a.schema.json"}"#).unwrap();

        assert_eq!(input.format(), &Some(DataFormat::Csv));
        assert_eq!(input.context(), &Some(AtContextMode::None));
        assert_eq!(input.schema(), &Some(SchemaSource::Local(PathBuf::from("a.schema.json"))));
    }

    #[test]
    fn reads_per_input_vars() {
        let input = parse(r#"{"source": "data/a.csv", "mapping": "m.json5", "vars": {"provider": "SenLab", "run": 7}}"#).unwrap();

        let vars = input.vars().as_ref().unwrap();
        assert_eq!(vars.get("provider"), Some(&serde_json::json!("SenLab")));
        assert_eq!(vars.get("run"), Some(&serde_json::json!(7)));
    }

    #[test]
    fn a_per_input_vars_map_round_trips_through_serialization() {
        let input = parse(r#"{"source": "a.csv", "mapping": "m.json5", "vars": {"valid_from": "2026-08-04T16:00:00Z"}}"#).unwrap();

        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(parse(&json).unwrap(), input);
    }

    #[test]
    fn declaring_both_mapping_and_mappings_is_rejected() {
        let error = parse(r#"{"source": "a.kml", "mapping": "m.json5", "mappings": [{"collection": "A", "mapping": "a.json5"}]}"#).unwrap_err();
        assert!(error.to_string().contains("not both"));
    }

    #[test]
    fn declaring_neither_mapping_nor_mappings_is_rejected() {
        assert!(parse(r#"{"source": "data/stations.csv"}"#).is_err());
    }

    #[test]
    fn an_empty_mappings_list_is_rejected() {
        assert!(parse(r#"{"source": "a.kml", "mappings": []}"#).is_err());
    }

    #[test]
    fn a_duplicate_collection_label_is_rejected() {
        let error =
            parse(r#"{"source": "a.kml", "mappings": [{"collection": "A", "mapping": "a.json5"}, {"collection": "A", "mapping": "b.json5"}]}"#).unwrap_err();
        assert!(error.to_string().contains("'A'"));
    }

    #[test]
    fn collections_under_a_non_collection_format_are_rejected() {
        let error = parse(r#"{"source": "a.csv", "format": "csv", "mappings": [{"collection": "A", "mapping": "a.json5"}]}"#).unwrap_err();
        assert!(error.to_string().contains("does not support multiple collections"));
    }

    #[test]
    fn collections_under_kml_are_accepted() {
        assert!(parse(r#"{"source": "a.kml", "format": "kml", "mappings": [{"collection": "A", "mapping": "a.json5"}]}"#).is_ok());
    }

    #[test]
    fn collections_with_an_omitted_format_defer_to_the_runtime_router() {
        assert!(parse(r#"{"source": "a.kml", "mappings": [{"collection": "A", "mapping": "a.json5"}]}"#).is_ok());
    }

    #[test]
    fn a_collections_input_round_trips_through_serialization() {
        let input = parse(r#"{"source": "a.kml", "format": "kml", "mappings": [{"collection": "Camera Area", "mapping": "a.json5"}, {"collection": "Flowcount", "mapping": "b.json5"}]}"#).unwrap();

        let json = serde_json::to_string(&input).unwrap();
        let restored = parse(&json).unwrap();
        assert_eq!(restored, input);
    }

    #[test]
    fn an_unrecognised_field_is_rejected() {
        assert!(parse(r#"{"source": "a.csv", "mapping": "m.json5", "unknownField": "x"}"#).is_err());
    }

    #[test]
    fn the_conflicting_binding_error_is_typed() {
        let raw = ManifestInputRaw {
            source: Input::Local(PathBuf::from("a.kml")),
            mapping: Some(PathBuf::from("m.json5")),
            mappings: Some(vec![CollectionMapping::new(CollectionName::from("A"), PathBuf::from("a.json5"))]),
            format: None,
            context: None,
            schema: None,
            vars: None,
        };
        assert!(matches!(ManifestInput::try_from(raw), Err(ManifestError::ConflictingMappingBinding)));
    }
}

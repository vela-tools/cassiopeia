use crate::error::Result;
use cassiopeia_collector::{file_extension::FileExtension, source::CollectorSource};
use cassiopeia_common::{context::mode::AtContextMode, format::DataFormat, request_header::RequestHeaders, schema_source::SchemaSource};
use cassiopeia_csv_dialect::statement::DialectStatement;
use cassiopeia_manifest::{input::ManifestInput, mapping_binding::MappingBinding};
use cassiopeia_mapping::template::run_variables::RunVariables;
use cassiopeia_sensorthings::service::SensorThingsService;
use serde_json::{Map, Value};

/// The extension used to name a downloaded source whose format is not declared.
const DEFAULT_EXTENSION: &str = "dat";

/// One input lane resolved from a manifest input: where to collect from and with which request
/// headers, its declared format, how its records are routed to mappings, and any per-input
/// `@context` override.
///
/// Each lane runs collector -> profiler -> ingestor -> expander independently before all lanes
/// merge at the resolver. A lane is read exactly once regardless of how many mappings its binding
/// carries, so a multi-collection source is never opened twice.
pub(crate) struct InputConfig {
    /// The source the lane's collector reads.
    pub(crate) source: LaneSource,
    /// The headers the collector attaches to every request that fetches the source.
    pub(crate) headers: RequestHeaders,
    /// The declared format, or `None` to auto-detect.
    pub(crate) format_override: Option<DataFormat>,
    /// What the input states about a CSV source's dialect; nothing when it states no `dialect`.
    pub(crate) dialect: DialectStatement,
    /// How this lane's records are routed to mappings.
    pub(crate) mapping_binding: MappingBinding,
    /// A per-input `@context` mode overriding the manifest-wide one.
    pub(crate) context_mode: Option<AtContextMode>,
    /// A per-input custom validation schema overriding the manifest-wide global schema for the types
    /// this lane produces. Resolved to an on-disk path once per cycle by schema resolution.
    pub(crate) schema: Option<SchemaSource>,
    /// This lane's effective run variables: the manifest-global map (already overlaid by CLI
    /// `--var`) overlaid by this input's own `vars`, carried beside every record the lane expands.
    /// Empty when the run declares none.
    pub(crate) variables: RunVariables,
}

/// Where a lane's records come from, which decides the collector that reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaneSource {
    /// Files, local or downloaded, read as they are.
    Files(CollectorSource),
    /// A SensorThings API server, read into a snapshot file every cycle.
    SensorThings(SensorThingsService),
}

/// Builds a lane configuration from one manifest input.
///
/// `effective_global` is the run's global run-level variables (the manifest's `vars` already overlaid
/// by any CLI `--var`); this lane's own `vars` overlay it, per-input winning on a shared name.
///
/// # Errors
///
/// Returns [`PipelineError`](crate::error::PipelineError) when the declared format's file extension
/// is invalid.
pub(crate) fn build_input_config(input: &ManifestInput, effective_global: &Map<String, Value>) -> Result<InputConfig> {
    let format_override = (*input.format()).and_then(DataFormat::to_option);
    let source = match input.sensorthings() {
        // The lane owns its service, while the manifest keeps its own.
        Some(service) => LaneSource::SensorThings(service.clone()),
        None => LaneSource::Files(CollectorSource::Single {
            // The lane owns its source, while the manifest keeps its own.
            input: input.source().clone(),
            extension: FileExtension::new(format_override.map_or(DEFAULT_EXTENSION, |format| format.extension()))?,
        }),
    };

    Ok(InputConfig {
        source,
        // The lane owns its headers, while the manifest keeps its own.
        headers: input.headers().clone(),
        format_override,
        // The lane owns its statement, while the manifest keeps its own.
        dialect: input.dialect().clone().unwrap_or_default(),
        mapping_binding: input.mapping_binding().clone(),
        context_mode: input.context().clone(),
        schema: input.schema().clone(),
        variables: RunVariables::new(merge_vars(effective_global, input.vars().as_ref())),
    })
}

/// Overlays an input's own variables onto the effective global map, the per-input value winning on a
/// shared name. The result is cloned since each lane owns its own map, which its records share.
fn merge_vars(effective_global: &Map<String, Value>, per_input: Option<&Map<String, Value>>) -> Map<String, Value> {
    let mut merged = effective_global.clone();
    if let Some(per_input) = per_input {
        for (name, value) in per_input {
            merged.insert(name.clone(), value.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use crate::input_config::{LaneSource, build_input_config};
    use cassiopeia_common::{
        format::DataFormat,
        input::Input,
        request_header::{RequestHeader, RequestHeaders},
    };
    use cassiopeia_csv_dialect::{dialect_byte::DialectByte, statement::DialectStatement};
    use cassiopeia_manifest::{input::ManifestInput, mapping_binding::MappingBinding};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, json};
    use std::{path::PathBuf, str::FromStr};

    /// Builds a manifest input over a local file source and the given declared format.
    fn manifest_input(format: DataFormat) -> ManifestInput {
        ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .format(format.to_option())
            .build()
    }

    #[test]
    fn a_remote_inputs_headers_carry_through_to_the_lane() {
        let headers = RequestHeaders::new(vec![RequestHeader::new("X-Api-Key", "k").unwrap()]);
        let input = ManifestInput::builder()
            .source(Input::from_str("https://api.example/records").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .build()
            .with_headers(headers.clone())
            .unwrap();

        let config = build_input_config(&input, &Map::new()).unwrap();

        assert_eq!(config.headers, headers);
    }

    #[test]
    fn an_input_without_headers_gives_its_lane_none() {
        let config = build_input_config(&manifest_input(DataFormat::Csv), &Map::new()).unwrap();

        assert!(config.headers.is_empty());
    }

    #[test]
    fn a_declared_format_carries_through_to_the_lane() {
        let config = build_input_config(&manifest_input(DataFormat::Csv), &Map::new()).unwrap();

        assert_eq!(config.format_override, Some(DataFormat::Csv));
        assert_eq!(
            config.mapping_binding,
            MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5")
            }
        );
    }

    #[test]
    fn a_stated_dialect_carries_through_to_the_lane() {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .format(Some(DataFormat::Csv))
            .dialect(Some(serde_json::from_str(r#"{"delimiter": ";"}"#).unwrap()))
            .build();

        let config = build_input_config(&input, &Map::new()).unwrap();

        assert_eq!(config.dialect.delimiter(), Some(DialectByte::SEMICOLON));
    }

    #[test]
    fn an_input_without_a_dialect_states_nothing_to_its_lane() {
        let config = build_input_config(&manifest_input(DataFormat::Csv), &Map::new()).unwrap();

        assert_eq!(config.dialect, DialectStatement::NOTHING);
    }

    #[test]
    fn an_auto_format_leaves_the_lane_without_an_override() {
        let config = build_input_config(&manifest_input(DataFormat::Auto), &Map::new()).unwrap();

        assert_eq!(config.format_override, None);
    }

    #[test]
    fn a_remote_source_builds_a_lane() {
        let input = ManifestInput::builder()
            .source(Input::from_str("http://example.com/data.json").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .format(DataFormat::Json.to_option())
            .build();

        assert_eq!(build_input_config(&input, &Map::new()).unwrap().format_override, Some(DataFormat::Json));
    }

    #[test]
    fn a_remote_sensorthings_input_gives_its_lane_a_server_to_read() {
        let input: ManifestInput =
            serde_json::from_str(r#"{"source": "https://h.example/v1.1", "format": "sensorthings", "mapping": "m.json5", "sensorthings": {"pageSize": 10}}"#)
                .unwrap();

        let config = build_input_config(&input, &Map::new()).unwrap();

        assert_eq!(config.source, LaneSource::SensorThings(input.sensorthings().clone().unwrap()));
        assert_eq!(config.format_override, Some(DataFormat::SensorThings));
    }

    #[test]
    fn a_local_snapshot_gives_its_lane_a_file_to_read() {
        let input: ManifestInput = serde_json::from_str(r#"{"source": "rivm.jsonl", "format": "sensorthings", "mapping": "m.json5"}"#).unwrap();

        let config = build_input_config(&input, &Map::new()).unwrap();

        assert!(matches!(config.source, LaneSource::Files(_)), "{:?}", config.source);
    }

    #[test]
    fn a_per_input_var_overrides_a_global_of_the_same_name() {
        let mut global = Map::new();
        global.insert("provider".to_string(), json!("Global"));
        global.insert("run".to_string(), json!("7"));

        let mut per_input = Map::new();
        per_input.insert("provider".to_string(), json!("Local"));

        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .vars(Some(per_input))
            .build();

        let config = build_input_config(&input, &global).unwrap();

        // The per-input value wins on the shared name; the global-only entry still carries through.
        assert_eq!(config.variables.as_value(), &json!({"provider": "Local", "run": "7"}));
    }

    #[test]
    fn a_lane_without_vars_carries_the_global_map_unchanged() {
        let mut global = Map::new();
        global.insert("valid_from".to_string(), json!("2026-08-04T16:00:00Z"));

        let config = build_input_config(&manifest_input(DataFormat::Csv), &global).unwrap();

        assert_eq!(config.variables.as_value(), &json!({"valid_from": "2026-08-04T16:00:00Z"}));
    }
}

use crate::error::Result;
use cassiopeia_collector::{file_extension::FileExtension, source::CollectorSource};
use cassiopeia_common::{context::mode::AtContextMode, format::DataFormat, schema_source::SchemaSource};
use cassiopeia_manifest::{input::ManifestInput, mapping_binding::MappingBinding};
use serde_json::{Map, Value};

/// The extension used to name a downloaded source whose format is not declared.
const DEFAULT_EXTENSION: &str = "dat";

/// One input lane resolved from a manifest input: where to collect from, its declared format, how
/// its records are routed to mappings, and any per-input `@context` override.
///
/// Each lane runs collector -> profiler -> ingestor -> expander independently before all lanes
/// merge at the resolver. A lane is read exactly once regardless of how many mappings its binding
/// carries, so a multi-collection source is never opened twice.
pub(crate) struct InputConfig {
    /// The source the collector reads.
    pub(crate) collector_source: CollectorSource,
    /// The declared format, or `None` to auto-detect.
    pub(crate) format_override: Option<DataFormat>,
    /// How this lane's records are routed to mappings.
    pub(crate) mapping_binding: MappingBinding,
    /// A per-input `@context` mode overriding the manifest-wide one.
    pub(crate) context_mode: Option<AtContextMode>,
    /// A per-input custom validation schema overriding the manifest-wide global schema for the types
    /// this lane produces. Resolved to an on-disk path once per cycle by schema resolution.
    pub(crate) schema: Option<SchemaSource>,
    /// This lane's effective run-level variables: the manifest-global map (already overlaid by CLI
    /// `--var`) overlaid by this input's own `vars`, injected into every record the lane expands
    /// under the reserved `vars` key. Empty when the run declares none.
    pub(crate) vars: Map<String, Value>,
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
    let extension = FileExtension::new(format_override.map_or(DEFAULT_EXTENSION, |format| format.extension()))?;
    let collector_source = CollectorSource::Single {
        input: input.source().clone(),
        extension,
    };

    Ok(InputConfig {
        collector_source,
        format_override,
        mapping_binding: input.mapping_binding().clone(),
        context_mode: input.context().clone(),
        schema: input.schema().clone(),
        vars: merge_vars(effective_global, input.vars().as_ref()),
    })
}

/// Overlays an input's own variables onto the effective global map, the per-input value winning on a
/// shared name. The result is cloned since it is stored on the lane and injected into every record.
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
    use crate::input_config::build_input_config;
    use cassiopeia_common::{format::DataFormat, input::Input};
    use cassiopeia_manifest::{input::ManifestInput, mapping_binding::MappingBinding};
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
        assert_eq!(config.vars.get("provider"), Some(&json!("Local")));
        assert_eq!(config.vars.get("run"), Some(&json!("7")));
    }

    #[test]
    fn a_lane_without_vars_carries_the_global_map_unchanged() {
        let mut global = Map::new();
        global.insert("valid_from".to_string(), json!("2026-08-04T16:00:00Z"));

        let config = build_input_config(&manifest_input(DataFormat::Csv), &global).unwrap();

        assert_eq!(config.vars.get("valid_from"), Some(&json!("2026-08-04T16:00:00Z")));
    }
}

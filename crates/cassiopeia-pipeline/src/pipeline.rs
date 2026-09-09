use crate::{
    error::{PipelineError, Result},
    input_config::{InputConfig, build_input_config},
    observer::RunObserver,
    pipeline_config::PipelineConfig,
};
use cassiopeia_common::{
    broker_operation::BrokerOperationKind,
    context::mode::AtContextMode,
    file_framing::FileFraming,
    representation::NgsiLdRepresentation,
    schema_source::SchemaSource,
    skip_null::NgsiLdSkipNull,
};
use cassiopeia_manifest::{
    failure_policy::FailurePolicy,
    manifest::Manifest,
    output::{destination::Destination, temporal::TemporalRepresentation, validation_mode::ValidationMode},
    schedule::{Schedule, retry::RetryPolicy},
};
use cassiopeia_reporter::reporter::Reporter;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

/// A configured, ready-to-run transformation pipeline built from a manifest.
///
/// A run reads every manifest input through its own collector -> profiler -> ingestor -> expander
/// lane, merges the lanes at the resolver, then extracts, transforms, validates, and writes. The
/// [`Schedule`] decides how often the whole thing repeats.
pub struct Pipeline {
    /// The input lanes resolved from the manifest.
    pub(crate) inputs: Vec<InputConfig>,
    /// Where and how entities are written.
    pub(crate) output: OutputConfig,
    /// The engine knobs: batching, stores, channels, and representations.
    pub(crate) config: PipelineConfig,
    /// When the run repeats and how it reacts to failure.
    pub(crate) schedule: ScheduleConfig,
    /// The run's execution context: reporting, cancellation, and observation.
    pub(crate) context: RunContext,
}

/// Where and how a run's entities are written.
pub(crate) struct OutputConfig {
    /// The manifest destination: a directory or a Context Broker.
    pub(crate) destination: Destination,
    /// The NGSI-LD representation for written entities, when the manifest sets one.
    pub(crate) representation: Option<NgsiLdRepresentation>,
    /// How null values in written entities are handled, when the manifest sets it.
    pub(crate) skip_null: Option<NgsiLdSkipNull>,
    /// Whether the run emits the full temporal series (`temporal.representation == "series"`), which
    /// selects the series store and inserts the fold stage. Absent temporal output leaves this false,
    /// the current-state default.
    pub(crate) series_representation: bool,
    /// The `@context` mode applied to every input that does not override it.
    pub(crate) global_context_mode: AtContextMode,
    /// Where a JSON validation report is written, when requested.
    pub(crate) validation_report_path: Option<PathBuf>,
    /// How strictly schema validation is enforced before an entity is written.
    pub(crate) validation_mode: ValidationMode,
    /// A custom validation schema applied to every entity type no per-input `schema` covers, when
    /// the run sets one. Resolved to an on-disk file per cycle; `None` leaves the convention alone.
    pub(crate) global_validation_schema: Option<SchemaSource>,
    /// The representation the validator serializes entities in, resolved CLI-over-manifest-over-default.
    pub(crate) validation_representation: NgsiLdRepresentation,
    /// Whether the validator drops null-valued attributes before checking, resolved the same way.
    pub(crate) validation_skip_null: NgsiLdSkipNull,
}

/// The validation settings a run supplies from the command line, each overriding the manifest.
///
/// These travel together because they are exactly the inline `--validation-*` flags: none of them
/// belong to a manifest run, where the manifest's own `output.validation` supplies the equivalents.
pub struct ValidationOverrides {
    /// Where a JSON validation report is written, when requested.
    pub report_path: Option<PathBuf>,
    /// The validation mode override, when given.
    pub mode: Option<ValidationMode>,
    /// The custom validation schema override, when given.
    pub schema: Option<SchemaSource>,
    /// The validation representation override, when given.
    pub representation: Option<NgsiLdRepresentation>,
    /// The validation skip-null override, when given.
    pub skip_null: Option<NgsiLdSkipNull>,
}

/// When a run repeats and how it reacts to a failed run.
pub(crate) struct ScheduleConfig {
    /// The manifest schedule, or `None` for a one-shot run.
    pub(crate) schedule: Option<Schedule>,
    /// What a failed run does to the schedule.
    pub(crate) failure_policy: FailurePolicy,
    /// How a failed run is retried before it counts as failed.
    pub(crate) retry: Option<RetryPolicy>,
}

/// The run's execution context: reporting, cancellation, and observation.
pub(crate) struct RunContext {
    /// The reporter every stage reports progress to.
    pub(crate) reporter: &'static dyn Reporter,
    /// The process-lifetime shutdown flag a signal handler sets, the source of cancellation.
    pub(crate) shutdown: &'static AtomicBool,
    /// The observer that receives the run's lifecycle events.
    pub(crate) observer: Arc<dyn RunObserver>,
}

impl Pipeline {
    /// Builds a pipeline from a validated manifest and the engine configuration.
    ///
    /// The manifest supplies the inputs, destination, schedule, and `@context` settings; `config`
    /// supplies the engine knobs. A manifest-level memory profile, when present, overrides the one in
    /// `config`.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`](crate::error::PipelineError) when an input's file extension is
    /// invalid.
    pub fn from_manifest(
        manifest: &Manifest,
        validation: ValidationOverrides,
        cli_vars: serde_json::Map<String, serde_json::Value>,
        mut config: PipelineConfig,
        reporter: &'static dyn Reporter,
        shutdown: &'static AtomicBool,
        observer: Arc<dyn RunObserver>,
    ) -> Result<Pipeline> {
        let ValidationOverrides {
            report_path,
            mode,
            schema,
            representation: cli_validation_representation,
            skip_null: cli_validation_skip_null,
        } = validation;

        if let Some(profile) = manifest.memory_profile() {
            config.memory_profile = *profile;
            config.apply_memory_profile();
        }

        // The run-level variables every lane starts from: the manifest's global `vars` overlaid by any
        // CLI `--var`, the command line winning as the explicit runtime override of the file. Each
        // lane then overlays its own per-input `vars` on top in `build_input_config`.
        let mut effective_global = manifest.vars().clone().unwrap_or_default();
        for (name, value) in cli_vars {
            effective_global.insert(name, value);
        }

        let inputs = manifest
            .inputs()
            .iter()
            .map(|input| build_input_config(input, &effective_global))
            .collect::<Result<Vec<_>>>()?;

        let output = manifest.output().as_ref();
        let destination = output.map_or_else(|| Destination::file(None, FileFraming::default()), |output| output.destination().clone());
        let representation = output.and_then(|output| *output.representation());
        let skip_null = output.and_then(|output| *output.skip_null());
        let series_representation = output
            .and_then(|output| output.temporal().as_ref())
            .is_some_and(|temporal| *temporal.representation() == TemporalRepresentation::Series);
        // A folded EntityTemporal only belongs at the broker's temporal endpoint, so a series run
        // against a Context Broker requires `operation: "temporal"`.
        if series_representation
            && let Destination::ContextBroker { operation, .. } = &destination
            && *operation != BrokerOperationKind::Temporal
        {
            return Err(PipelineError::SeriesRequiresTemporalOperation);
        }
        let global_context_mode = output
            .and_then(|output| output.context().clone())
            .unwrap_or_else(|| config.context_mode.clone());

        // Every validation knob resolves CLI-over-manifest-over-default, reading the manifest side
        // from the nested `output.validation` object.
        let manifest_validation = output.and_then(|output| output.validation().as_ref());
        // A CLI override wins over the manifest, which in turn wins over the fail-when-schema default.
        let validation_mode = mode
            .or_else(|| manifest_validation.and_then(|validation| *validation.mode()))
            .unwrap_or_default();
        // Validation defaults to the simplified (key-values) form, the shape a Smart Data Model
        // schema describes, not the domain `Default` (Normalized) representation that writing uses.
        let validation_representation = cli_validation_representation
            .or_else(|| manifest_validation.and_then(|validation| *validation.representation()))
            .unwrap_or(NgsiLdRepresentation::Simplified);
        let validation_skip_null = cli_validation_skip_null
            .or_else(|| manifest_validation.and_then(|validation| *validation.skip_null()))
            .unwrap_or(NgsiLdSkipNull::Skip);
        // Inline runs pass the flag here and the manifest carries none; a `--manifest` run passes
        // `None` and the manifest's `output.validation.schema` wins, the same precedence the mode
        // above follows.
        let global_validation_schema = schema.or_else(|| manifest_validation.and_then(|validation| validation.schema().clone()));
        let validation_report_path = report_path.or_else(|| manifest_validation.and_then(|validation| validation.report().clone()));

        let schedule = manifest.schedule().clone();
        // The run-level policy governs a one-shot run and a scheduled run alike; its absence defaults
        // to aborting, so a single failed cycle surfaces as an error to the caller.
        let failure_policy = manifest.on_failure().unwrap_or_default();
        let retry = schedule.as_ref().and_then(|schedule| *schedule.retry());

        Ok(Pipeline {
            inputs,
            output: OutputConfig {
                destination,
                representation,
                skip_null,
                series_representation,
                global_context_mode,
                validation_report_path,
                validation_mode,
                global_validation_schema,
                validation_representation,
                validation_skip_null,
            },
            config,
            schedule: ScheduleConfig {
                schedule,
                failure_policy,
                retry,
            },
            context: RunContext { reporter, shutdown, observer },
        })
    }

    /// Runs the pipeline, driven by its schedule.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`](crate::error::PipelineError) when a run fails and the failure policy
    /// is [`FailurePolicy::Abort`].
    pub fn run(&mut self) -> Result<()> {
        let schedule = self.schedule.schedule.take();
        self.run_schedule(schedule)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::PipelineError,
        observer::{NoopObserver, RunObserver},
        pipeline::{Pipeline, ValidationOverrides},
        pipeline_config::{ExtractionParallelism, PipelineConfig},
    };
    use cassiopeia_common::{
        channel::ChannelPolicy,
        context::mode::AtContextMode,
        file_framing::FileFraming,
        input::Input,
        memory_profile::MemoryProfile,
        pipeline_mode::PipelineMode,
        representation::NgsiLdRepresentation,
        schema_source::SchemaSource,
        store_kind::StoreKind,
        user_agent::UserAgent,
    };
    use cassiopeia_manifest::{
        failure_policy::FailurePolicy,
        input::ManifestInput,
        inputs::Inputs,
        manifest::Manifest,
        mapping_binding::MappingBinding,
        output::{ManifestOutput, destination::Destination, validation::ManifestValidation, validation_mode::ValidationMode},
        version::Version,
    };
    use cassiopeia_reporter::{backend::noop::NoopReporter, reporter::Reporter};
    use std::{
        num::NonZeroUsize,
        path::PathBuf,
        str::FromStr,
        sync::{Arc, atomic::AtomicBool},
    };

    static NOOP: NoopReporter = NoopReporter::new();
    static SHUTDOWN: AtomicBool = AtomicBool::new(false);

    /// The engine knobs a from-manifest test runs under; the values match the crate defaults and are
    /// irrelevant to the validation-mode resolution under test.
    fn config() -> PipelineConfig {
        PipelineConfig {
            batch_size: 10_000,
            extraction: ExtractionParallelism::Parallel,
            mode: PipelineMode::Batch,
            entity_store: StoreKind::DashMap,
            relationship_store: StoreKind::DashMap,
            schemas_folder: PathBuf::from("schemas"),
            context_mode: AtContextMode::Default,
            default_user_agent: UserAgent::from("test".to_owned()),
            channel_policy: ChannelPolicy::Bounded(NonZeroUsize::new(64).unwrap()),
            memory_profile: MemoryProfile::Default,
        }
    }

    /// Builds a one-input manifest whose output carries the given validation mode.
    fn manifest(validation: Option<ValidationMode>) -> Manifest {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .build();
        let output = ManifestOutput::builder()
            .destination(Destination::file(None, FileFraming::default()))
            .validation(validation.map(|mode| ManifestValidation::builder().mode(Some(mode)).build()))
            .build();

        Manifest::builder()
            .version(Version::V1)
            .inputs(Inputs::new(vec![input]).unwrap())
            .output(Some(output))
            .build()
    }

    /// Resolves the validation mode a pipeline is built with from a CLI override and a manifest value.
    fn resolved_mode(cli: Option<ValidationMode>, manifest_mode: Option<ValidationMode>) -> ValidationMode {
        let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);
        let pipeline = Pipeline::from_manifest(
            &manifest(manifest_mode),
            ValidationOverrides {
                report_path: None,
                mode: cli,
                schema: None,
                representation: None,
                skip_null: None,
            },
            serde_json::Map::new(),
            config(),
            &NOOP as &'static dyn Reporter,
            &SHUTDOWN,
            observer,
        )
        .unwrap();

        pipeline.output.validation_mode
    }

    /// Builds a one-input manifest whose output carries the given custom validation schema.
    fn manifest_with_schema(schema: Option<SchemaSource>) -> Manifest {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .build();
        let output = ManifestOutput::builder()
            .destination(Destination::file(None, FileFraming::default()))
            .validation(schema.map(|source| ManifestValidation::builder().schema(Some(source)).build()))
            .build();

        Manifest::builder()
            .version(Version::V1)
            .inputs(Inputs::new(vec![input]).unwrap())
            .output(Some(output))
            .build()
    }

    /// Resolves the custom validation schema a pipeline is built with from a flag and a manifest value.
    fn resolved_schema(param: Option<SchemaSource>, manifest_schema: Option<SchemaSource>) -> Option<SchemaSource> {
        let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);
        let pipeline = Pipeline::from_manifest(
            &manifest_with_schema(manifest_schema),
            ValidationOverrides {
                report_path: None,
                mode: None,
                schema: param,
                representation: None,
                skip_null: None,
            },
            serde_json::Map::new(),
            config(),
            &NOOP as &'static dyn Reporter,
            &SHUTDOWN,
            observer,
        )
        .unwrap();

        pipeline.output.global_validation_schema
    }

    #[test]
    fn a_cli_override_wins_over_the_manifest_mode() {
        assert_eq!(resolved_mode(Some(ValidationMode::Warn), Some(ValidationMode::Fail)), ValidationMode::Warn);
    }

    #[test]
    fn the_manifest_mode_governs_when_no_cli_override_is_given() {
        assert_eq!(resolved_mode(None, Some(ValidationMode::Fail)), ValidationMode::Fail);
    }

    #[test]
    fn the_fail_when_schema_default_governs_when_neither_states_a_mode() {
        assert_eq!(resolved_mode(None, None), ValidationMode::FailWhenSchema);
    }

    #[test]
    fn a_param_validation_schema_governs_without_a_manifest_value() {
        let param = SchemaSource::Local(PathBuf::from("cli.json"));
        assert_eq!(resolved_schema(Some(param.clone()), None), Some(param));
    }

    #[test]
    fn the_manifest_output_schema_governs_without_a_param() {
        let manifest_schema = SchemaSource::Local(PathBuf::from("manifest.json"));
        assert_eq!(resolved_schema(None, Some(manifest_schema.clone())), Some(manifest_schema));
    }

    #[test]
    fn neither_a_param_nor_a_manifest_schema_yields_none() {
        assert_eq!(resolved_schema(None, None), None);
    }

    /// Builds a one-input manifest whose output validation carries the given representation.
    fn manifest_with_representation(representation: Option<NgsiLdRepresentation>) -> Manifest {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .build();
        let output = ManifestOutput::builder()
            .destination(Destination::file(None, FileFraming::default()))
            .validation(representation.map(|representation| ManifestValidation::builder().representation(Some(representation)).build()))
            .build();

        Manifest::builder()
            .version(Version::V1)
            .inputs(Inputs::new(vec![input]).unwrap())
            .output(Some(output))
            .build()
    }

    /// Resolves the validation representation a pipeline is built with from a CLI flag and a manifest.
    fn resolved_representation(cli: Option<NgsiLdRepresentation>, manifest_representation: Option<NgsiLdRepresentation>) -> NgsiLdRepresentation {
        let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);
        let pipeline = Pipeline::from_manifest(
            &manifest_with_representation(manifest_representation),
            ValidationOverrides {
                report_path: None,
                mode: None,
                schema: None,
                representation: cli,
                skip_null: None,
            },
            serde_json::Map::new(),
            config(),
            &NOOP as &'static dyn Reporter,
            &SHUTDOWN,
            observer,
        )
        .unwrap();

        pipeline.output.validation_representation
    }

    #[test]
    fn a_cli_flag_wins_over_the_manifest_validation_representation() {
        assert_eq!(
            resolved_representation(Some(NgsiLdRepresentation::Concise), Some(NgsiLdRepresentation::Normalized)),
            NgsiLdRepresentation::Concise
        );
    }

    #[test]
    fn the_manifest_validation_representation_governs_without_a_cli_flag() {
        assert_eq!(
            resolved_representation(None, Some(NgsiLdRepresentation::Normalized)),
            NgsiLdRepresentation::Normalized
        );
    }

    #[test]
    fn validation_representation_defaults_to_simplified() {
        assert_eq!(resolved_representation(None, None), NgsiLdRepresentation::Simplified);
    }

    /// Builds a one-input manifest carrying the given run-level failure policy and no schedule.
    fn manifest_with_failure_policy(policy: Option<FailurePolicy>) -> Manifest {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .build();

        Manifest::builder()
            .version(Version::V1)
            .inputs(Inputs::new(vec![input]).unwrap())
            .on_failure(policy)
            .build()
    }

    /// Resolves the failure policy a pipeline is built with from a manifest's run-level value.
    fn resolved_failure_policy(policy: Option<FailurePolicy>) -> FailurePolicy {
        let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);
        let pipeline = Pipeline::from_manifest(
            &manifest_with_failure_policy(policy),
            ValidationOverrides {
                report_path: None,
                mode: None,
                schema: None,
                representation: None,
                skip_null: None,
            },
            serde_json::Map::new(),
            config(),
            &NOOP as &'static dyn Reporter,
            &SHUTDOWN,
            observer,
        )
        .unwrap();

        pipeline.schedule.failure_policy
    }

    /// Builds a one-input manifest carrying the given (already parsed) output section.
    fn manifest_with_output(output: ManifestOutput) -> Manifest {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .build();

        Manifest::builder()
            .version(Version::V1)
            .inputs(Inputs::new(vec![input]).unwrap())
            .output(Some(output))
            .build()
    }

    /// Runs `from_manifest` over a manifest carrying `output`, with no CLI overrides.
    fn build_with_output(output: ManifestOutput) -> Result<Pipeline, PipelineError> {
        let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);
        Pipeline::from_manifest(
            &manifest_with_output(output),
            ValidationOverrides {
                report_path: None,
                mode: None,
                schema: None,
                representation: None,
                skip_null: None,
            },
            serde_json::Map::new(),
            config(),
            &NOOP as &'static dyn Reporter,
            &SHUTDOWN,
            observer,
        )
    }

    #[test]
    fn a_series_representation_against_a_broker_without_the_temporal_operation_is_rejected() {
        let output: ManifestOutput =
            serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:9090/", "temporal": {"representation": "series"}}"#).unwrap();

        assert!(matches!(build_with_output(output), Err(PipelineError::SeriesRequiresTemporalOperation)));
    }

    #[test]
    fn a_series_representation_against_a_broker_with_the_temporal_operation_is_accepted() {
        let output: ManifestOutput = serde_json::from_str(
            r#"{"target": "context-broker", "url": "http://localhost:9090/", "operation": "temporal", "temporal": {"representation": "series"}}"#,
        )
        .unwrap();

        assert!(build_with_output(output).is_ok());
    }

    #[test]
    fn a_series_representation_against_a_file_is_accepted() {
        let output: ManifestOutput = serde_json::from_str(r#"{"target": "file", "directory": "out", "temporal": {"representation": "series"}}"#).unwrap();

        assert!(build_with_output(output).is_ok());
    }

    #[test]
    fn an_absent_run_level_policy_defaults_to_abort() {
        assert_eq!(resolved_failure_policy(None), FailurePolicy::Abort);
    }

    #[test]
    fn a_stated_run_level_policy_governs_the_run() {
        assert_eq!(resolved_failure_policy(Some(FailurePolicy::Continue)), FailurePolicy::Continue);
        assert_eq!(resolved_failure_policy(Some(FailurePolicy::Ignore)), FailurePolicy::Ignore);
    }

    /// Builds a one-input manifest carrying the given global and per-input `vars`.
    fn manifest_with_vars(
        global: Option<serde_json::Map<String, serde_json::Value>>,
        per_input: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Manifest {
        let input = ManifestInput::builder()
            .source(Input::from_str("data.csv").unwrap())
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mapping.json5"),
            })
            .vars(per_input)
            .build();

        Manifest::builder()
            .version(Version::V1)
            .inputs(Inputs::new(vec![input]).unwrap())
            .vars(global)
            .build()
    }

    /// Resolves the first lane's effective run-level variables from a CLI, global, and per-input map.
    fn resolved_vars(
        cli: serde_json::Map<String, serde_json::Value>,
        global: Option<serde_json::Map<String, serde_json::Value>>,
        per_input: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let observer: Arc<dyn RunObserver> = Arc::new(NoopObserver);
        let pipeline = Pipeline::from_manifest(
            &manifest_with_vars(global, per_input),
            ValidationOverrides {
                report_path: None,
                mode: None,
                schema: None,
                representation: None,
                skip_null: None,
            },
            cli,
            config(),
            &NOOP as &'static dyn Reporter,
            &SHUTDOWN,
            observer,
        )
        .unwrap();

        pipeline.inputs.into_iter().next().unwrap().vars
    }

    /// A single-entry `vars` map, the shape each precedence layer is exercised with.
    fn vars(name: &str, value: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        map.insert(name.to_string(), serde_json::Value::String(value.to_string()));
        map
    }

    #[test]
    fn a_manifest_global_var_reaches_the_lane() {
        let resolved = resolved_vars(serde_json::Map::new(), Some(vars("valid_from", "2026-08-04T16:00:00Z")), None);

        assert_eq!(resolved.get("valid_from"), Some(&serde_json::json!("2026-08-04T16:00:00Z")));
    }

    #[test]
    fn a_cli_var_overrides_a_manifest_global_var() {
        let resolved = resolved_vars(vars("valid_from", "cli"), Some(vars("valid_from", "manifest")), None);

        assert_eq!(resolved.get("valid_from"), Some(&serde_json::json!("cli")));
    }

    #[test]
    fn a_per_input_var_overrides_the_cli_value() {
        let resolved = resolved_vars(
            vars("valid_from", "cli"),
            Some(vars("valid_from", "manifest")),
            Some(vars("valid_from", "input")),
        );

        assert_eq!(resolved.get("valid_from"), Some(&serde_json::json!("input")));
    }
}

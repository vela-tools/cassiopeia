//! Turns the `map` and `manifest generate` command-line arguments into a manifest.
//!
//! Routing principle: every flag that shapes *this run* (where output goes: the destination and
//! its representation, skip-null, tenant, user-agent, `@context`; how strictly it validates; and
//! when and how often it runs) is written into the manifest here. Flags that tune the *engine*
//! itself (validator representation and skip-null, store backends, the memory profile) never touch
//! the manifest; they become [`ConfigOverride`](cassiopeia_configuration::overrides::ConfigOverride)s
//! in `config_overrides` instead. The two surfaces stay disjoint so a run's description and the
//! installation's tuning cannot bleed into each other.

use crate::{
    cli::{broker::BrokerArgs, context::CliValidationError, manifest::ManifestGenArgs, map::MapArgs, validator::ValidatorArgs, writer::WriterArgs},
    error::Result,
    schedule_build::schedule_from_args,
};
use cassiopeia_common::{
    broker_atomicity::BrokerAtomicity,
    broker_header::BrokerHeaders,
    context::mode::AtContextMode,
    destination_kind::DestinationKind,
    file_framing::FileFraming,
    run_var::run_vars_to_map,
};
use cassiopeia_manifest::{
    input::ManifestInput,
    inputs::Inputs,
    manifest::Manifest,
    mapping_binding::MappingBinding,
    output::{
        ManifestOutput,
        context_delivery::ContextDelivery,
        destination::Destination,
        temporal::{ManifestTemporal, TemporalRepresentation},
        validation::ManifestValidation,
    },
    version::Version,
};
use std::path::PathBuf;

/// Builds an in-memory manifest from the `map` command's input, output, and schedule arguments.
///
/// Only reached when `--manifest` is absent, so `--input` and `--mapping` are present by clap's
/// `required_unless_present` rule; the fallible unwraps guard that invariant without panicking.
pub fn manifest_from_map_args(args: &MapArgs) -> Result<Manifest> {
    let source = args.input.clone().ok_or(CliValidationError::MissingInput)?;
    let mapping = args.mapping.clone().ok_or(CliValidationError::MissingMapping)?;

    let input = ManifestInput::builder()
        .source(source)
        .mapping_binding(MappingBinding::Single { mapping })
        .format(args.format_type.to_option())
        .build();
    let inputs = Inputs::new(vec![input])?;

    let context = args.context.resolve()?;
    let output = build_output(&args.writer, context)?;
    let schedule = schedule_from_args(&args.schedule);

    // An inline run has no manifest file to carry `vars`, so any `--var` is baked in as the run's
    // global variables here; an empty set leaves the field off.
    let vars = run_vars_to_map(&args.var);

    Ok(Manifest::builder()
        .version(Version::V1)
        .inputs(inputs)
        .output(Some(output))
        .schedule(schedule)
        .on_failure(args.on_failure)
        .vars((!vars.is_empty()).then_some(vars))
        .build())
}

/// Builds a manifest from the `manifest generate` arguments, to be written to a file.
pub fn manifest_from_gen_args(args: &ManifestGenArgs) -> Result<Manifest> {
    let input = ManifestInput::builder()
        .source(args.input.clone())
        .mapping_binding(MappingBinding::Single { mapping: args.mapping.clone() })
        .format(args.format_type.to_option())
        .build();
    let inputs = Inputs::new(vec![input])?;

    let output = build_gen_output(args)?;
    let schedule = schedule_from_args(&args.schedule);

    Ok(Manifest::builder()
        .version(Version::V1)
        .inputs(inputs)
        .output(output)
        .schedule(schedule)
        .on_failure(args.on_failure)
        .build())
}

/// Builds the manifest output from the `map` command's writer arguments and resolved `@context`.
fn build_output(writer: &WriterArgs, context: Option<AtContextMode>) -> Result<ManifestOutput> {
    let destination = build_destination(writer)?;

    Ok(ManifestOutput::builder()
        .destination(destination)
        .representation(writer.writer_representation)
        .skip_null(writer.writer_skip_null)
        .context(context)
        // Opt-in: the flag's absence leaves `temporal` off the manifest, like the other output flags.
        .temporal(temporal_output(writer.temporal_representation))
        .build())
}

/// Builds the optional `output.temporal` object from the `--temporal-representation` flag.
fn temporal_output(representation: Option<TemporalRepresentation>) -> Option<ManifestTemporal> {
    representation.map(|representation| ManifestTemporal::builder().representation(representation).build())
}

/// Builds the destination from the `map` command's writer arguments.
fn build_destination(writer: &WriterArgs) -> Result<Destination> {
    destination(writer.writer_type, writer.output.clone(), writer.framing, &writer.broker)
}

/// Builds a destination from a resolved writer kind, file-output settings, and the broker flags.
///
/// This is the single bridge both `map` and `manifest generate` route through, so the two commands
/// map their (identical) broker flags the same way.
fn destination(kind: DestinationKind, output: Option<PathBuf>, framing: FileFraming, broker: &BrokerArgs) -> Result<Destination> {
    match kind {
        DestinationKind::File => Ok(Destination::file(output, framing)),
        DestinationKind::ContextBroker => broker_destination(broker),
    }
}

/// Builds a Context Broker destination from the shared broker flags.
fn broker_destination(broker: &BrokerArgs) -> Result<Destination> {
    let url = broker.broker_url.clone().ok_or(CliValidationError::MissingBrokerUrl)?;

    Ok(Destination::ContextBroker {
        url,
        tenant: broker.tenant.clone(),
        user_agent: broker.user_agent.clone(),
        headers: BrokerHeaders::new(broker.headers.clone()),
        context_delivery: ContextDelivery::from_link_header(broker.link_header),
        operation: broker.broker_operation,
        upsert_mode: broker.upsert_mode,
        attribute_overwrite: broker.attribute_overwrite,
        atomicity: BrokerAtomicity::from_flag(broker.atomic),
    })
}

/// Builds the optional manifest output from the `manifest generate` arguments.
///
/// A generated manifest carries an output section only when a writer target was named; otherwise the
/// section is omitted so the manifest inherits the run-time default. The validation mode rides on
/// the output section, so it is only recorded when a destination is present.
fn build_gen_output(args: &ManifestGenArgs) -> Result<Option<ManifestOutput>> {
    let Some(target) = args.writer_type else {
        return Ok(None);
    };

    let context = args.context.resolve()?;
    let destination = destination(target, args.output.clone(), args.framing, &args.broker)?;

    Ok(Some(
        ManifestOutput::builder()
            .destination(destination)
            .representation(args.writer_representation)
            .skip_null(args.writer_skip_null)
            .context(context)
            .validation(validation_from_args(&args.validator))
            // Opt-in: the flag's absence leaves `temporal` off the manifest, like the other output flags.
            .temporal(temporal_output(args.temporal_representation))
            .build(),
    ))
}

/// Builds the nested `output.validation` object from the validation flags, or `None` when no
/// validation flag was given, so a generated manifest records an empty `validation: {}` for none.
fn validation_from_args(validator: &ValidatorArgs) -> Option<ManifestValidation> {
    let any = validator.validation_mode.is_some()
        || validator.schema.is_some()
        || validator.representation.is_some()
        || validator.skip_null.is_some()
        || validator.report.is_some();

    any.then(|| {
        ManifestValidation::builder()
            .mode(validator.validation_mode)
            .schema(validator.schema.clone())
            .representation(validator.representation)
            .skip_null(validator.skip_null)
            .report(validator.report.clone())
            .build()
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::{
            Cli,
            Commands,
            manifest::{ManifestGenArgs, ManifestSubcommand},
            map::MapArgs,
        },
        manifest_build::{build_destination, build_gen_output, manifest_from_gen_args, manifest_from_map_args},
    };
    use cassiopeia_common::{
        attribute_overwrite::AttributeOverwrite,
        broker_atomicity::BrokerAtomicity,
        broker_operation::BrokerOperationKind,
        file_framing::FileFraming,
        schema_source::SchemaSource,
        upsert_mode::UpsertMode,
    };
    use cassiopeia_manifest::{
        failure_policy::FailurePolicy,
        output::{
            ManifestOutput,
            destination::Destination,
            temporal::{ManifestTemporal, TemporalRepresentation},
            validation_mode::ValidationMode,
        },
    };
    use clap::Parser;
    use std::path::PathBuf;

    /// Parses a `map` command line, returning its flattened arguments.
    fn map_args(command_line: &[&str]) -> MapArgs {
        let Commands::Map(args) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the map command");
        };
        *args
    }

    /// Parses a `manifest generate` command line, returning its arguments.
    fn gen_args(command_line: &[&str]) -> ManifestGenArgs {
        let Commands::Manifest(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the manifest command");
        };
        let ManifestSubcommand::Generate(args) = command.command;
        args
    }

    #[test]
    fn a_file_run_maps_its_framing_into_the_destination() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--output",
            "out",
            "--framing",
            "line-delimited",
        ]);

        match build_destination(&args.writer).unwrap() {
            Destination::File { framing, .. } => assert_eq!(framing, FileFraming::LineDelimited),
            Destination::ContextBroker { .. } => panic!("expected a file destination"),
        }
    }

    #[test]
    fn a_file_run_defaults_to_array_framing() {
        let args = map_args(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--writer", "file", "--output", "out"]);

        match build_destination(&args.writer).unwrap() {
            Destination::File { framing, .. } => assert_eq!(framing, FileFraming::Array),
            Destination::ContextBroker { .. } => panic!("expected a file destination"),
        }
    }

    #[test]
    fn a_broker_run_maps_its_operation_and_atomicity_into_the_destination() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
            "--broker-operation",
            "temporal",
            "--atomic",
        ]);

        match build_destination(&args.writer).unwrap() {
            Destination::ContextBroker { operation, atomicity, .. } => {
                assert_eq!(operation, BrokerOperationKind::Temporal);
                assert_eq!(atomicity, BrokerAtomicity::Atomic);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn a_broker_run_maps_an_updating_upsert_into_the_destination() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
            "--broker-operation",
            "upsert",
            "--upsert-mode",
            "update",
        ]);

        match build_destination(&args.writer).unwrap() {
            Destination::ContextBroker { operation, upsert_mode, .. } => {
                assert_eq!(operation, BrokerOperationKind::Upsert);
                assert_eq!(upsert_mode, UpsertMode::Update);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn a_broker_run_maps_a_no_overwrite_update_into_the_destination() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
            "--broker-operation",
            "update",
            "--attribute-overwrite",
            "no-overwrite",
        ]);

        match build_destination(&args.writer).unwrap() {
            Destination::ContextBroker {
                operation,
                attribute_overwrite,
                ..
            } => {
                assert_eq!(operation, BrokerOperationKind::Update);
                assert_eq!(attribute_overwrite, AttributeOverwrite::NoOverwrite);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn upsert_mode_on_a_file_run_is_rejected() {
        let result = Cli::try_parse_from([
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--output",
            "out",
            "--upsert-mode",
            "update",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn a_broker_run_defaults_to_upserting_and_streaming() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
        ]);

        match build_destination(&args.writer).unwrap() {
            Destination::ContextBroker {
                operation,
                upsert_mode,
                attribute_overwrite,
                atomicity,
                ..
            } => {
                assert_eq!(operation, BrokerOperationKind::Upsert);
                assert_eq!(upsert_mode, UpsertMode::Replace);
                assert_eq!(attribute_overwrite, AttributeOverwrite::Overwrite);
                assert_eq!(atomicity, BrokerAtomicity::Streaming);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn framing_on_a_broker_run_is_rejected() {
        let result = Cli::try_parse_from([
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
            "--framing",
            "line-delimited",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn atomic_on_a_file_run_is_rejected() {
        let result = Cli::try_parse_from([
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--output",
            "out",
            "--atomic",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn the_generate_command_mirrors_the_file_framing_flag() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--directory",
            "out",
            "--framing",
            "line-delimited",
        ]);

        let output = build_gen_output(&args).unwrap().unwrap();
        match output.destination() {
            Destination::File { framing, .. } => assert_eq!(*framing, FileFraming::LineDelimited),
            Destination::ContextBroker { .. } => panic!("expected a file destination"),
        }
    }

    #[test]
    fn the_generate_command_mirrors_the_broker_flags() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
            "--broker-operation",
            "update",
            "--attribute-overwrite",
            "no-overwrite",
            "--atomic",
        ]);

        let output = build_gen_output(&args).unwrap().unwrap();
        match output.destination() {
            Destination::ContextBroker {
                operation,
                attribute_overwrite,
                atomicity,
                ..
            } => {
                assert_eq!(*operation, BrokerOperationKind::Update);
                assert_eq!(*attribute_overwrite, AttributeOverwrite::NoOverwrite);
                assert_eq!(*atomicity, BrokerAtomicity::Atomic);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn the_generate_command_records_a_schedule() {
        let args = gen_args(&["cassiopeia", "manifest", "generate", "-i", "data.csv", "-m", "map.json5", "--every", "30s"]);

        let manifest = manifest_from_gen_args(&args).unwrap();

        assert!(manifest.schedule().is_some());
    }

    #[test]
    fn the_generate_command_records_a_validation_mode_on_the_output() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--directory",
            "out",
            "--validation-mode",
            "fail",
        ]);

        let output = build_gen_output(&args).unwrap().unwrap();

        assert_eq!(output.validation().as_ref().unwrap().mode(), &Some(ValidationMode::Fail));
    }

    #[test]
    fn the_generate_command_records_a_validation_schema_on_the_output() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--directory",
            "out",
            "--validation-schema",
            "exoplanet.schema.json",
        ]);

        let output = build_gen_output(&args).unwrap().unwrap();

        assert_eq!(
            output.validation().as_ref().unwrap().schema(),
            &Some(SchemaSource::Local(PathBuf::from("exoplanet.schema.json")))
        );
    }

    #[test]
    fn a_map_validation_schema_flag_parses_a_local_path() {
        let args = map_args(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--validation-schema", "./x.json"]);

        assert_eq!(args.validator.schema, Some(SchemaSource::Local(PathBuf::from("./x.json"))));
    }

    #[test]
    fn a_map_validation_schema_flag_parses_a_remote_url() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--validation-schema",
            "https://example.org/x.schema.json",
        ]);

        let Some(SchemaSource::Remote(url)) = args.validator.schema else {
            panic!("expected a remote schema source");
        };
        assert_eq!(url.as_str(), "https://example.org/x.schema.json");
    }

    #[test]
    fn the_generate_command_carries_a_broker_user_agent() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:1026/",
            "--user-agent",
            "acme/2.0",
        ]);

        let output = build_gen_output(&args).unwrap().unwrap();
        match output.destination() {
            Destination::ContextBroker { user_agent, .. } => {
                assert_eq!(user_agent.as_ref().map(ToString::to_string), Some("acme/2.0".to_string()));
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn a_map_run_shaping_flag_reaches_the_manifest_output() {
        // `--writer-representation` shapes the run's output, so it must land on the built manifest.
        let args = map_args(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--writer-representation", "concise"]);

        let manifest = manifest_from_map_args(&args).unwrap();
        let representation = manifest.output().as_ref().and_then(|output: &ManifestOutput| *output.representation());

        assert!(representation.is_some());
    }

    /// Resolves the temporal representation a `map` command line records on its manifest output.
    fn map_temporal(command_line: &[&str]) -> Option<TemporalRepresentation> {
        let manifest = manifest_from_map_args(&map_args(command_line)).unwrap();
        manifest
            .output()
            .as_ref()
            .and_then(|output: &ManifestOutput| output.temporal().as_ref().map(ManifestTemporal::representation).copied())
    }

    #[test]
    fn a_series_representation_is_recorded_on_a_file_run() {
        let temporal = map_temporal(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--output",
            "out",
            "--temporal-representation",
            "series",
        ]);

        assert_eq!(temporal, Some(TemporalRepresentation::Series));
    }

    #[test]
    fn a_series_representation_is_recorded_on_a_broker_run() {
        let temporal = map_temporal(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "context-broker",
            "--broker-url",
            "http://localhost:9090/",
            "--temporal-representation",
            "series",
        ]);

        assert_eq!(temporal, Some(TemporalRepresentation::Series));
    }

    #[test]
    fn without_the_temporal_flag_the_manifest_output_leaves_it_unset() {
        let temporal = map_temporal(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--writer", "file", "--output", "out"]);

        assert_eq!(temporal, None);
    }

    #[test]
    fn a_temporal_representation_is_accepted_alongside_the_file_writer() {
        // `--temporal-representation` is a general output shape, not a broker-only flag, so it is valid
        // with a file writer.
        let result = Cli::try_parse_from([
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--output",
            "out",
            "--temporal-representation",
            "series",
        ]);

        assert!(result.is_ok());
    }

    /// Resolves the run-level failure policy a `map` command line records on its manifest.
    fn map_on_failure(command_line: &[&str]) -> Option<FailurePolicy> {
        *manifest_from_map_args(&map_args(command_line)).unwrap().on_failure()
    }

    #[test]
    fn a_continue_on_failure_flag_is_recorded_on_the_manifest() {
        let policy = map_on_failure(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--on-failure", "continue"]);

        assert_eq!(policy, Some(FailurePolicy::Continue));
    }

    #[test]
    fn an_ignore_on_failure_flag_is_recorded_on_the_manifest() {
        let policy = map_on_failure(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--on-failure", "ignore"]);

        assert_eq!(policy, Some(FailurePolicy::Ignore));
    }

    #[test]
    fn an_absent_on_failure_flag_leaves_the_manifest_policy_unset() {
        let policy = map_on_failure(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5"]);

        assert_eq!(policy, None);
    }

    #[test]
    fn an_inline_var_flag_is_baked_into_the_manifest_global_vars() {
        let args = map_args(&[
            "cassiopeia",
            "map",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--var",
            "valid_from=2026-08-04T16:00:00Z",
        ]);

        let manifest = manifest_from_map_args(&args).unwrap();

        let vars = manifest.vars().as_ref().unwrap();
        assert_eq!(vars.get("valid_from").and_then(|value| value.as_str()), Some("2026-08-04T16:00:00Z"));
    }

    #[test]
    fn no_var_flag_leaves_the_manifest_global_vars_unset() {
        let args = map_args(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5"]);

        let manifest = manifest_from_map_args(&args).unwrap();

        assert_eq!(manifest.vars(), &None);
    }

    #[test]
    fn the_generate_command_bakes_the_on_failure_policy() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--on-failure",
            "ignore",
        ]);

        let manifest = manifest_from_gen_args(&args).unwrap();

        assert_eq!(*manifest.on_failure(), Some(FailurePolicy::Ignore));
    }

    #[test]
    fn the_generate_command_mirrors_the_temporal_representation() {
        let args = gen_args(&[
            "cassiopeia",
            "manifest",
            "generate",
            "-i",
            "data.csv",
            "-m",
            "map.json5",
            "--writer",
            "file",
            "--directory",
            "out",
            "--temporal-representation",
            "series",
        ]);

        let output = build_gen_output(&args).unwrap().unwrap();

        assert_eq!(
            output.temporal().as_ref().map(ManifestTemporal::representation),
            Some(&TemporalRepresentation::Series)
        );
    }
}

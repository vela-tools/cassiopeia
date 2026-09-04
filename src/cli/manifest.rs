use crate::cli::{broker::BrokerArgs, context::ContextArgs, schedule::ScheduleArgs, validator::ValidatorArgs};
use cassiopeia_common::{
    destination_kind::DestinationKind,
    file_framing::FileFraming,
    format::DataFormat,
    input::Input,
    representation::NgsiLdRepresentation,
    skip_null::NgsiLdSkipNull,
};
use cassiopeia_manifest::{failure_policy::FailurePolicy, output::temporal::TemporalRepresentation};
use clap::{Args, Parser, Subcommand};
use std::{path::PathBuf, str::FromStr};

/// The `manifest` command group.
#[derive(Parser, Debug)]
pub struct ManifestCommand {
    /// The manifest operation to perform.
    #[command(subcommand)]
    pub command: ManifestSubcommand,
}

/// Operations over manifest files.
#[derive(Subcommand, Debug)]
pub enum ManifestSubcommand {
    /// Generate a manifest file from command-line arguments.
    #[command(arg_required_else_help = true)]
    Generate(ManifestGenArgs),
}

/// Arguments for generating a manifest file.
#[derive(Args, Debug)]
#[command(next_line_help = true)]
pub struct ManifestGenArgs {
    /// Data source file path or URL.
    #[arg(short, long, value_parser = Input::from_str, help_heading = "Input")]
    pub input: Input,

    /// JSON5 mapping file defining the transformation rules.
    #[arg(short, long, help_heading = "Input", value_name = "FILE")]
    pub mapping: PathBuf,

    /// Data format of the input source.
    #[arg(
        short = 't',
        long = "type",
        value_enum,
        help_heading = "Input",
        value_name = "FORMAT",
        default_value_t = DataFormat::Auto,
    )]
    pub format_type: DataFormat,

    /// How the run reacts to a failed cycle and what it exits with: abort (stop and exit non-zero),
    /// continue (run the rest, still exit non-zero), or ignore (run the rest, exit zero). Defaults to
    /// abort. Recorded on the generated manifest as `onFailure`.
    #[arg(long = "on-failure", help_heading = "Behavior", value_name = "MODE")]
    pub on_failure: Option<FailurePolicy>,

    /// Output path for the generated manifest.
    #[arg(short = 'M', long, default_value = "manifest.json5", help_heading = "Behavior", value_name = "FILE")]
    pub manifest_output: PathBuf,

    /// Where to write results: the file system or a Context Broker.
    #[arg(short, long = "writer", value_enum, help_heading = "Output", value_name = "TYPE")]
    pub writer_type: Option<DestinationKind>,

    /// Output directory (for the file writer).
    #[arg(short = 'o', long = "directory", help_heading = "Output", value_name = "DIRECTORY")]
    pub output: Option<PathBuf>,

    /// File framing: a JSON array or line-delimited entities (file writer only).
    #[arg(
        long,
        value_enum,
        default_value_t = FileFraming::Array,
        conflicts_with = "broker_url",
        help_heading = "Output",
        value_name = "FRAMING"
    )]
    pub framing: FileFraming,

    /// NGSI-LD representation for written entities.
    #[arg(long = "representation", value_enum, help_heading = "Output", value_name = "REPRESENTATION")]
    pub writer_representation: Option<NgsiLdRepresentation>,

    /// How to handle null values in written entities.
    #[arg(long = "skip-null", value_enum, help_heading = "Output", value_name = "SKIPNULL")]
    pub writer_skip_null: Option<NgsiLdSkipNull>,

    /// Temporal output shape. `series` folds each id's observations into one temporal entity with
    /// instance arrays (ETSI GS CIM 009 v1.9.1 clause 5.2.20); absent writes current-state (one entity
    /// per id, latest per attribute). A general output shape, valid for the file writer and the broker.
    #[arg(long = "temporal-representation", value_name = "REPRESENTATION", help_heading = "Output")]
    pub temporal_representation: Option<TemporalRepresentation>,

    #[command(flatten)]
    pub validator: ValidatorArgs,

    #[command(flatten)]
    pub broker: BrokerArgs,

    #[command(flatten)]
    pub context: ContextArgs,

    #[command(flatten)]
    pub schedule: ScheduleArgs,
}

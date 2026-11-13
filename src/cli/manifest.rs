use crate::cli::{
    broker::BrokerArgs,
    context::ContextArgs,
    failure::FailureArgs,
    input_read::InputReadArgs,
    output_format::OutputFormatArgs,
    schedule::ScheduleArgs,
    validator::ValidatorArgs,
};
use cassiopeia_common::{destination_kind::DestinationKind, input::Input};
use clap::{Arg, Args, Parser, Subcommand};
use std::{path::PathBuf, str::FromStr};

/// Gives the shared writer-representation flag the hidden `--representation` alias on this command
/// alone: the spelling `manifest generate` used before it matched `map`, kept while scripts migrate.
///
/// Applied through `mut_args`, which keeps every argument in place, where `mut_arg` would move the
/// flag to the end of the help.
fn retired_representation_alias(arg: Arg) -> Arg {
    if arg.get_id() == "writer_representation" {
        arg.alias("representation")
    } else {
        arg
    }
}

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
///
/// Every run-shaping flag is spelled as on `map`, and the flags whose meaning is the same on both
/// commands come from the argument structs they share. `--input`, `--mapping`, `--writer`, and
/// `--output` stay declared per command: `map` may take them from `--manifest` instead and writes to
/// a file unless told otherwise, while a generated manifest names its input outright and records an
/// output section only when `--writer` is given.
#[derive(Args, Debug)]
#[command(next_line_help = true)]
#[command(mut_args(retired_representation_alias))]
pub struct ManifestGenArgs {
    /// Data source file path or URL.
    #[arg(short, long, value_parser = Input::from_str, help_heading = "Input")]
    pub input: Input,

    /// JSON5 mapping file defining the transformation rules.
    #[arg(short, long, help_heading = "Input", value_name = "FILE")]
    pub mapping: PathBuf,

    #[command(flatten)]
    pub input_read: InputReadArgs,

    #[command(flatten)]
    pub failure: FailureArgs,

    /// Output path for the generated manifest.
    #[arg(short = 'M', long, default_value = "manifest.json5", help_heading = "Behavior", value_name = "FILE")]
    pub manifest_output: PathBuf,

    /// Where to write results: the file system or a Context Broker.
    #[arg(short, long = "writer", value_enum, help_heading = "Output", value_name = "WRITER")]
    pub writer_type: Option<DestinationKind>,

    /// Output directory (for the file writer).
    // `--directory` is how this command spelled `--output` before it matched `map`; it stays a hidden
    // alias while scripts migrate.
    #[arg(short, long, alias = "directory", help_heading = "Output", value_name = "DIRECTORY")]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub format: OutputFormatArgs,

    #[command(flatten)]
    pub validator: ValidatorArgs,

    #[command(flatten)]
    pub broker: BrokerArgs,

    #[command(flatten)]
    pub context: ContextArgs,

    #[command(flatten)]
    pub schedule: ScheduleArgs,
}

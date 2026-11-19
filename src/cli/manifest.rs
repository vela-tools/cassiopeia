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

/// Ends the help of each shared flag this command records on the generated manifest with the field
/// it is recorded as.
///
/// The flags come from argument structs `map` shares, and `map` records nothing, so their shared
/// help cannot say it. Applied through `mut_args` for the same reason as
/// [`retired_representation_alias`].
fn recorded_on_manifest(arg: Arg) -> Arg {
    let field = match arg.get_id().as_str() {
        "input_headers" => "the input's `headers`",
        "on_failure" => "`onFailure`",
        _unrecorded => return arg,
    };
    match arg.get_help().map(ToString::to_string) {
        Some(help) => arg.help(format!("{help}. Recorded on the generated manifest as {field}")),
        None => arg,
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
#[command(mut_args(recorded_on_manifest))]
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

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use clap::CommandFactory;
    use pretty_assertions::assert_eq;

    /// The help of the argument `id` on the subcommand that `path` names.
    fn help_of(path: &[&str], id: &str) -> String {
        let command = Cli::command();
        let subcommand = path.iter().fold(&command, |command, name| command.find_subcommand(name).unwrap());
        subcommand
            .get_arguments()
            .find(|arg| arg.get_id() == id)
            .unwrap()
            .get_help()
            .unwrap()
            .to_string()
    }

    #[test]
    fn generate_help_says_the_input_headers_are_recorded_as_the_inputs_headers() {
        assert_eq!(
            help_of(&["manifest", "generate"], "input_headers"),
            format!(
                "{}. Recorded on the generated manifest as the input's `headers`",
                help_of(&["map"], "input_headers")
            )
        );
    }

    #[test]
    fn generate_help_says_the_failure_policy_is_recorded_as_on_failure() {
        assert_eq!(
            help_of(&["manifest", "generate"], "on_failure"),
            format!("{}. Recorded on the generated manifest as `onFailure`", help_of(&["map"], "on_failure"))
        );
    }

    #[test]
    fn map_help_of_the_shared_flags_names_no_generated_manifest() {
        for id in ["input_headers", "on_failure"] {
            assert!(!help_of(&["map"], id).contains("generated manifest"), "{id}");
        }
    }

    #[test]
    fn generate_help_of_any_other_shared_flag_matches_map() {
        assert_eq!(help_of(&["manifest", "generate"], "format_type"), help_of(&["map"], "format_type"));
    }
}

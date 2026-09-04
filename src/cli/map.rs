use crate::cli::{
    context::{CliValidationError, ContextArgs},
    schedule::ScheduleArgs,
    validator::ValidatorArgs,
    writer::WriterArgs,
};
use cassiopeia_common::{channel::ChannelPolicy, format::DataFormat, input::Input, pipeline_mode::PipelineMode, run_var::RunVar, store_kind::StoreKind};
use cassiopeia_manifest::failure_policy::FailurePolicy;
use clap::{ArgAction, Args};
use std::{num::NonZeroUsize, path::PathBuf, str::FromStr};

/// Parses a channel capacity, accepting `none` for an unbounded handoff.
///
/// A capacity has to be able to go both ways from the command line: a configured bound must be
/// removable for one run, not only raised or lowered. Naming the unbounded case as a value rather
/// than as the absence of the flag keeps the three states (untouched, unbounded, and bounded at N)
/// distinct.
fn parse_channel_capacity(value: &str) -> Result<ChannelPolicy, String> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(ChannelPolicy::Unbounded);
    }
    value
        .parse::<NonZeroUsize>()
        .map(ChannelPolicy::Bounded)
        .map_err(|error| format!("expected a positive count or 'none': {error}"))
}

/// Every inline run-shaping argument `--manifest` is mutually exclusive with.
///
/// These are the flags that describe *what* a run does: its input, mapping and type, and every
/// output, context, validation, schedule, and failure-policy flag, all of which a manifest supplies
/// instead. The ids are clap argument ids (the field names of the flattened argument structs). Engine
/// knobs that merely tune *how* the pipeline executes are deliberately absent, so they stay usable
/// with a manifest.
const INLINE_RUN_FLAGS: [&str; 35] = [
    "input",
    "mapping",
    "format_type",
    "writer_type",
    "output",
    "broker_url",
    "framing",
    "broker_operation",
    "upsert_mode",
    "attribute_overwrite",
    "atomic",
    "writer_representation",
    "writer_skip_null",
    "temporal_representation",
    "tenant",
    "user_agent",
    "headers",
    "link_header",
    "kind",
    "url",
    "file",
    "every",
    "cron",
    "at",
    "repeat",
    "duration",
    "jitter",
    "retry",
    "retry_backoff",
    "on_failure",
    "representation",
    "skip_null",
    "validation_mode",
    "schema",
    "report",
];

/// Arguments for the `map` command, which runs the transformation pipeline.
#[derive(Args, Debug)]
#[command(next_line_help = true)]
pub struct MapArgs {
    /// Path to a manifest file; it supplies the run's input, mapping, output, validation, and
    /// schedule settings, replacing the corresponding command-line options.
    ///
    /// A manifest describes the whole run, so it is mutually exclusive with every inline
    /// run-shaping flag: input, mapping, and type, and every output, context, validation, schedule,
    /// and failure-policy flag. Engine knobs that are not part of a run's description (the pipeline
    /// mode, the store backends, the memory profile) remain valid alongside it.
    #[arg(short = 'M', long, conflicts_with_all = INLINE_RUN_FLAGS, help_heading = "Input")]
    pub manifest: Option<PathBuf>,

    /// Data source file path or URL.
    #[arg(short, long, value_parser = Input::from_str, required_unless_present = "manifest", help_heading = "Input")]
    pub input: Option<Input>,

    /// JSON5 mapping file defining the transformation rules.
    #[arg(short, long, required_unless_present = "manifest", help_heading = "Input", value_name = "FILE")]
    pub mapping: Option<PathBuf>,

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

    /// A run-level variable, written as `KEY=VALUE`; repeat for several. Every mapping in the run can
    /// read it as `{{ vars.KEY }}`. Valid alongside `--manifest`, where it overrides a manifest global
    /// `vars` of the same name (a per-input `vars` still wins). A repeated key keeps the last value.
    #[arg(long = "var", action = ArgAction::Append, help_heading = "Input", value_name = "KEY=VALUE")]
    pub var: Vec<RunVar>,

    /// How the run reacts to a failed cycle and what it exits with: abort (stop and exit non-zero),
    /// continue (run the rest, still exit non-zero), or ignore (run the rest, exit zero). Defaults to
    /// abort.
    #[arg(long = "on-failure", help_heading = "Behavior", value_name = "MODE")]
    pub on_failure: Option<FailurePolicy>,

    /// Pipeline processing mode: batch (buffered parallel) or single (immediate per-item).
    #[arg(long, value_enum, help_heading = "Behavior", default_value_t = PipelineMode::Batch)]
    pub mode: PipelineMode,

    #[command(flatten)]
    pub validator: ValidatorArgs,

    #[command(flatten)]
    pub writer: WriterArgs,

    #[command(flatten)]
    pub context: ContextArgs,

    /// Entity store backend for the resolver (overrides config).
    #[arg(short = 'e', long, value_enum, help_heading = "Behavior", value_name = "STORE")]
    pub entity_store: Option<StoreKind>,

    /// Relationship store backend for the resolver (overrides config).
    #[arg(long, value_enum, help_heading = "Behavior", value_name = "STORE")]
    pub relationship_store: Option<StoreKind>,

    /// Use the low-memory pipeline profile for large feeds on constrained hosts.
    #[arg(long, help_heading = "Behavior", action = ArgAction::SetTrue)]
    pub low_memory: bool,

    /// How many records or entities a stage hands to the next in one batch (overrides config).
    #[arg(long, help_heading = "Behavior", value_name = "COUNT")]
    pub batch_size: Option<NonZeroUsize>,

    /// How many batches may wait between two stages before the producer blocks, or `none` for
    /// unbounded handoffs (overrides config).
    #[arg(long, help_heading = "Behavior", value_name = "COUNT|none", value_parser = parse_channel_capacity)]
    pub channel_capacity: Option<ChannelPolicy>,

    /// How many worker threads the parallel stages share (overrides config). Defaults to the
    /// machine's performance-core count on hybrid hardware, every logical processor otherwise.
    #[arg(long, help_heading = "Behavior", value_name = "COUNT")]
    pub threads: Option<NonZeroUsize>,

    #[command(flatten)]
    pub schedule: ScheduleArgs,
}

impl MapArgs {
    /// Validates the argument combinations clap cannot express on its own.
    pub fn validate(&self) -> Result<(), CliValidationError> {
        self.context.resolve()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Commands};
    use cassiopeia_common::{channel::ChannelPolicy, run_var::run_vars_to_map};
    use clap::Parser;
    use std::num::NonZeroUsize;

    /// Parses a `map` command line, returning its flattened arguments.
    fn map_args(command_line: &[&str]) -> super::MapArgs {
        let Commands::Map(args) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the map command");
        };
        *args
    }

    #[test]
    fn a_manifest_run_accepts_a_bare_manifest_path() {
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5"]);

        assert!(result.is_ok());
    }

    #[test]
    fn a_manifest_run_rejects_an_inline_output_flag() {
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--writer", "file", "--output", "out"]);

        assert!(result.is_err());
    }

    #[test]
    fn a_manifest_run_rejects_an_inline_schedule_flag() {
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--every", "30s"]);

        assert!(result.is_err());
    }

    #[test]
    fn a_manifest_run_rejects_an_inline_validation_flag() {
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--validation-mode", "fail"]);

        assert!(result.is_err());
    }

    #[test]
    fn a_manifest_run_rejects_an_inline_on_failure_flag() {
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--on-failure", "ignore"]);

        assert!(result.is_err());
    }

    #[test]
    fn a_manifest_run_rejects_an_inline_validation_schema_flag() {
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--validation-schema", "x.json"]);

        assert!(result.is_err());
    }

    #[test]
    fn a_manifest_run_still_accepts_an_engine_knob() {
        // The pipeline mode tunes execution, not the run's description, so it is valid with a
        // manifest.
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--mode", "single"]);

        assert!(result.is_ok());
    }

    #[test]
    fn a_manifest_run_still_accepts_a_var_flag() {
        // A run-level variable is a value the run supplies to its mappings, not a description of the
        // run's shape, so it stays valid alongside a manifest.
        let result = Cli::try_parse_from(["cassiopeia", "map", "--manifest", "run.json5", "--var", "valid_from=2026-08-04T16:00:00Z"]);

        assert!(result.is_ok());
    }

    #[test]
    fn repeated_var_flags_keep_the_last_value() {
        let args = map_args(&["cassiopeia", "map", "--manifest", "run.json5", "--var", "a=1", "--var", "a=2"]);

        let map = run_vars_to_map(&args.var);
        assert_eq!(map.get("a").and_then(|value| value.as_str()), Some("2"));
    }

    #[test]
    fn the_engine_knobs_parse_and_stay_valid_alongside_a_manifest() {
        // Batch size, channel capacity and thread count tune how the pipeline executes rather than
        // describing the run, so a manifest must not conflict with them.
        let args = map_args(&[
            "cassiopeia",
            "map",
            "--manifest",
            "run.json5",
            "--batch-size",
            "5000",
            "--channel-capacity",
            "16",
            "--threads",
            "8",
        ]);

        assert_eq!(args.batch_size.map(NonZeroUsize::get), Some(5000));
        assert_eq!(args.channel_capacity, Some(ChannelPolicy::Bounded(NonZeroUsize::new(16).unwrap())));
        assert_eq!(args.threads.map(NonZeroUsize::get), Some(8));
    }

    #[test]
    fn an_unbounded_channel_capacity_is_spelled_none() {
        let args = map_args(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--channel-capacity", "none"]);

        // The flag was given, and what it selected is "no bound".
        assert_eq!(args.channel_capacity, Some(ChannelPolicy::Unbounded));
    }

    #[test]
    fn a_zero_or_unparseable_engine_knob_is_rejected() {
        for flag in ["--batch-size", "--threads", "--channel-capacity"] {
            let result = Cli::try_parse_from(["cassiopeia", "map", "-i", "d.csv", "-m", "m.json5", flag, "0"]);
            assert!(result.is_err(), "{flag} must reject a zero count");
        }
        assert!(Cli::try_parse_from(["cassiopeia", "map", "-i", "d.csv", "-m", "m.json5", "--channel-capacity", "lots"]).is_err());
    }

    #[test]
    fn a_var_flag_parses_alongside_inline_input() {
        let args = map_args(&["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "--var", "provider=SenLab"]);

        let map = run_vars_to_map(&args.var);
        assert_eq!(map.get("provider").and_then(|value| value.as_str()), Some("SenLab"));
    }
}

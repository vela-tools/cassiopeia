use cassiopeia_manifest::failure_policy::FailurePolicy;
use clap::Args;

/// How a run reacts to a failed cycle and what it exits with.
///
/// Shared by the `map` and `manifest generate` commands, so the two command surfaces cannot drift
/// apart.
#[derive(Args, Debug, PartialEq, Eq)]
pub struct FailureArgs {
    /// How the run reacts to a failed cycle and what it exits with: abort (stop and exit non-zero),
    /// continue (run the rest, still exit non-zero), or ignore (run the rest, exit zero). Defaults to
    /// abort.
    #[arg(long = "on-failure", help_heading = "Behavior", value_name = "MODE")]
    pub on_failure: Option<FailurePolicy>,
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Commands, failure::FailureArgs, manifest::ManifestSubcommand};
    use cassiopeia_manifest::failure_policy::FailurePolicy;
    use clap::Parser;
    use pretty_assertions::assert_eq;

    fn map_failure(flags: &[&str]) -> FailureArgs {
        let command_line = ["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "-o", "out"].iter().chain(flags);
        let Commands::Map(args) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the map command");
        };
        args.failure
    }

    fn generate_failure(flags: &[&str]) -> FailureArgs {
        let command_line = ["cassiopeia", "manifest", "generate", "-i", "data.csv", "-m", "map.json5"].iter().chain(flags);
        let Commands::Manifest(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the manifest command");
        };
        let ManifestSubcommand::Generate(args) = command.command;
        args.failure
    }

    #[test]
    fn map_and_generate_read_the_same_failure_policy() {
        let flags = ["--on-failure", "continue"];
        let expected = FailureArgs {
            on_failure: Some(FailurePolicy::Continue),
        };

        assert_eq!(map_failure(&flags), expected);
        assert_eq!(generate_failure(&flags), expected);
    }

    #[test]
    fn an_absent_failure_flag_leaves_the_policy_unset_on_both_commands() {
        assert_eq!(map_failure(&[]), FailureArgs { on_failure: None });
        assert_eq!(generate_failure(&[]), FailureArgs { on_failure: None });
    }
}

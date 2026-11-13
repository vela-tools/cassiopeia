use cassiopeia_common::{format::DataFormat, request_header::RequestHeader};
use clap::{ArgAction, Args};

/// How a run reads its input: the headers a remote fetch sends and the format the content is parsed
/// as.
///
/// Shared by the `map` and `manifest generate` commands, so the two command surfaces cannot drift
/// apart.
#[derive(Args, Debug, PartialEq, Eq)]
pub struct InputReadArgs {
    /// Extra HTTP header sent when fetching a remote input, written as `Name: Value`; repeat for
    /// several. Typically carries an API key, e.g. `--input-header "X-Api-Key: <key>"`. Refused for a
    /// local input.
    #[arg(long = "input-header", action = ArgAction::Append, help_heading = "Input", value_name = "NAME: VALUE")]
    pub input_headers: Vec<RequestHeader>,

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
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Commands, input_read::InputReadArgs, manifest::ManifestSubcommand};
    use cassiopeia_common::{format::DataFormat, request_header::RequestHeader};
    use clap::Parser;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    const READ_FLAGS: [&str; 4] = ["--input-header", "X-Api-Key: k", "--type", "csv"];

    fn map_input_read(flags: &[&str]) -> InputReadArgs {
        let command_line = ["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "-o", "out"].iter().chain(flags);
        let Commands::Map(args) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the map command");
        };
        args.input_read
    }

    fn generate_input_read(flags: &[&str]) -> InputReadArgs {
        let command_line = ["cassiopeia", "manifest", "generate", "-i", "data.csv", "-m", "map.json5"].iter().chain(flags);
        let Commands::Manifest(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the manifest command");
        };
        let ManifestSubcommand::Generate(args) = command.command;
        args.input_read
    }

    #[test]
    fn map_and_generate_read_the_same_input_flags_into_the_same_values() {
        let expected = InputReadArgs {
            input_headers: vec![RequestHeader::from_str("X-Api-Key: k").unwrap()],
            format_type: DataFormat::Csv,
        };

        assert_eq!(map_input_read(&READ_FLAGS), expected);
        assert_eq!(generate_input_read(&READ_FLAGS), expected);
    }

    #[test]
    fn absent_input_flags_send_no_header_and_detect_the_format_on_both_commands() {
        let expected = InputReadArgs {
            input_headers: Vec::new(),
            format_type: DataFormat::Auto,
        };

        assert_eq!(map_input_read(&[]), expected);
        assert_eq!(generate_input_read(&[]), expected);
    }
}

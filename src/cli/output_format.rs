use cassiopeia_common::{file_framing::FileFraming, representation::NgsiLdRepresentation};
use cassiopeia_manifest::output::temporal::TemporalRepresentation;
use clap::Args;

/// How a run formats the entities it writes, wherever they go: their NGSI-LD representation, their
/// temporal shape, and the framing of the files that hold them.
///
/// Shared by the `map` and `manifest generate` commands, so the two command surfaces cannot drift
/// apart. The framing conflicts with the broker URL, present under that id in both parent commands.
#[derive(Args, Debug, PartialEq, Eq)]
pub struct OutputFormatArgs {
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
    #[arg(long, value_enum, help_heading = "Output", value_name = "REPRESENTATION")]
    pub writer_representation: Option<NgsiLdRepresentation>,

    /// Temporal output shape. `series` folds each id's observations into one temporal entity with
    /// instance arrays (ETSI GS CIM 009 v1.9.1 clause 5.2.20); absent writes current-state (one entity
    /// per id, holding per mapping the record with the latest `observedAt`; `--broker-operation temporal`
    /// sends each observation on its own instead). A general output shape, valid for the file writer
    /// and the broker.
    #[arg(long = "temporal-representation", value_name = "REPRESENTATION", help_heading = "Output")]
    pub temporal_representation: Option<TemporalRepresentation>,
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Commands, manifest::ManifestSubcommand, output_format::OutputFormatArgs};
    use cassiopeia_common::{file_framing::FileFraming, representation::NgsiLdRepresentation};
    use cassiopeia_manifest::output::temporal::TemporalRepresentation;
    use clap::Parser;
    use pretty_assertions::assert_eq;

    const FORMAT_FLAGS: [&str; 6] = [
        "--framing",
        "line-delimited",
        "--writer-representation",
        "concise",
        "--temporal-representation",
        "series",
    ];

    fn map_format(flags: &[&str]) -> OutputFormatArgs {
        let command_line = ["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5", "-o", "out"].iter().chain(flags);
        let Commands::Map(args) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the map command");
        };
        args.writer.format
    }

    fn generate_format(flags: &[&str]) -> OutputFormatArgs {
        let command_line = ["cassiopeia", "manifest", "generate", "-i", "data.csv", "-m", "map.json5"].iter().chain(flags);
        let Commands::Manifest(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the manifest command");
        };
        let ManifestSubcommand::Generate(args) = command.command;
        args.format
    }

    #[test]
    fn map_and_generate_read_the_same_format_flags_into_the_same_values() {
        let expected = OutputFormatArgs {
            framing: FileFraming::LineDelimited,
            writer_representation: Some(NgsiLdRepresentation::Concise),
            temporal_representation: Some(TemporalRepresentation::Series),
        };

        assert_eq!(map_format(&FORMAT_FLAGS), expected);
        assert_eq!(generate_format(&FORMAT_FLAGS), expected);
    }

    #[test]
    fn absent_format_flags_give_an_array_framing_and_no_representation_on_both_commands() {
        let expected = OutputFormatArgs {
            framing: FileFraming::Array,
            writer_representation: None,
            temporal_representation: None,
        };

        assert_eq!(map_format(&[]), expected);
        assert_eq!(generate_format(&[]), expected);
    }
}

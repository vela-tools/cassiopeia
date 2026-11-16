use crate::cli::{input_read::InputHeaderArgs, sensorthings_cursor::CursorCommand};
use cassiopeia_sensorthings::{
    service_root::ServiceRoot,
    settings::stream_filter::{StreamFilter, StreamFilterError},
    stream_kind::StreamKind,
};
use clap::{Args, Parser, Subcommand};

/// The `sensorthings` command group.
#[derive(Parser, Debug)]
pub struct SensorThingsCommand {
    /// The SensorThings operation to perform.
    #[command(subcommand)]
    pub command: SensorThingsSubcommand,
}

/// Operations on SensorThings API servers.
#[derive(Subcommand, Debug)]
pub enum SensorThingsSubcommand {
    /// List the readings a SensorThings API server offers.
    ///
    /// Prints one row per `ObservedProperty` name, the name a mapping reads as `readings.<name>`, with
    /// the units of its readings, the UN/CEFACT codes `cefact_unit` resolves them to (`unknown` for
    /// one it does not resolve), the number of its Datastreams, the number of `MultiDatastreams` with
    /// an element of the name, and the number of Things that hold more than one reading of the name.
    /// `MultiDatastreams` are listed when the server's service root lists them.
    #[command(arg_required_else_help = true)]
    Readings(ReadingsArgs),

    /// Inspect and reset the cursors of incremental reads.
    #[command(arg_required_else_help = true)]
    Cursor(CursorCommand),
}

/// The arguments of `sensorthings readings`.
#[derive(Args, Debug)]
pub struct ReadingsArgs {
    /// The server's service root, ending with the version it serves, such as
    /// `https://api-samenmeten.rivm.nl/v1.0`. It carries no credentials: send them with
    /// `--input-header`.
    #[arg(value_name = "SERVICE_ROOT")]
    pub root: ServiceRoot,

    /// An `OData` `$filter` over Datastreams that selects the Datastreams listed, such as
    /// `ObservedProperty/name eq 'pm10'`; every Datastream when absent.
    #[arg(long, value_name = "EXPRESSION", value_parser = datastream_filter)]
    pub datastreams: Option<StreamFilter>,

    /// An `OData` `$filter` over `MultiDatastreams` that selects the `MultiDatastreams` listed, such as
    /// `startswith(name,'ws-')`; every `MultiDatastream` when absent.
    #[arg(long, value_name = "EXPRESSION", value_parser = multi_datastream_filter)]
    pub multi_datastreams: Option<StreamFilter>,

    #[command(flatten)]
    pub headers: InputHeaderArgs,
}

/// Reads a `--datastreams` filter, checked as the `datastreams` setting is.
fn datastream_filter(text: &str) -> Result<StreamFilter, StreamFilterError> {
    StreamFilter::new(StreamKind::Datastream, text.to_owned())
}

/// Reads a `--multi-datastreams` filter, checked as the `multiDatastreams` setting is.
fn multi_datastream_filter(text: &str) -> Result<StreamFilter, StreamFilterError> {
    StreamFilter::new(StreamKind::MultiDatastream, text.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::cli::{
        Cli,
        Commands,
        sensorthings::{ReadingsArgs, SensorThingsSubcommand},
    };
    use clap::Parser;
    use pretty_assertions::assert_eq;

    fn readings_args(arguments: &[&str]) -> ReadingsArgs {
        let command_line = ["cassiopeia", "sensorthings", "readings"].iter().chain(arguments);
        let Commands::SensorThings(command) = Cli::try_parse_from(command_line).unwrap().command else {
            panic!("expected the sensorthings command");
        };
        let SensorThingsSubcommand::Readings(args) = command.command else {
            panic!("expected the readings command");
        };
        args
    }

    #[test]
    fn the_readings_command_reads_its_root_filter_and_headers() {
        let args = readings_args(&[
            "https://api-samenmeten.rivm.nl/v1.0/",
            "--datastreams",
            "startswith(name,'ILM_I68-')",
            "--input-header",
            "Authorization: Bearer t",
        ]);

        assert_eq!(args.root.to_string(), "https://api-samenmeten.rivm.nl/v1.0");
        assert_eq!(
            args.datastreams.map(|filter| filter.to_string()),
            Some("startswith(name,'ILM_I68-')".to_owned())
        );
        assert_eq!(args.headers.input_headers.len(), 1);
    }

    #[test]
    fn without_options_every_data_stream_is_listed_without_extra_headers() {
        let args = readings_args(&["https://h.example/v1.1"]);

        assert_eq!(args.datastreams, None);
        assert_eq!(args.headers.input_headers, Vec::new());
    }

    #[test]
    fn a_root_that_is_no_service_root_is_refused_with_the_reason_a_manifest_gets() {
        let error = Cli::try_parse_from(["cassiopeia", "sensorthings", "readings", "https://h.example/api"]).unwrap_err();

        assert!(
            error.to_string().contains("a service root ends with the version it serves, such as `/v1.1`"),
            "{error}"
        );
    }

    #[test]
    fn a_service_root_with_credentials_is_refused() {
        let error = Cli::try_parse_from(["cassiopeia", "sensorthings", "readings", "https://u:p@h.example/v1.1"]).unwrap_err();

        assert!(error.to_string().contains("a service root carries no user name or password"), "{error}");
    }

    #[test]
    fn a_blank_filter_is_refused() {
        let error = Cli::try_parse_from(["cassiopeia", "sensorthings", "readings", "https://h.example/v1.1", "--datastreams", " "]).unwrap_err();

        assert!(error.to_string().contains("not blank text"), "{error}");
    }
}

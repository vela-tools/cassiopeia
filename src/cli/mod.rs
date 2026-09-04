use crate::cli::{
    config::ConfigCommand,
    manifest::ManifestCommand,
    map::MapArgs,
    mapping::MappingCommand,
    schema::SchemaCommand,
    sdm::SdmCommand,
    version::{ABOUT, long_version, short_version},
};
use cassiopeia_cli::clap_theme::THEME;
use cassiopeia_diagnostic::verbosity::Verbosity;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod broker;
pub mod config;
pub mod context;
pub mod manifest;
pub mod map;
pub mod mapping;
pub mod schedule;
pub mod schema;
pub mod sdm;
pub mod validator;
pub mod version;
pub mod writer;

/// The Cassiopeia command-line interface: a data mapper that transforms source files into NGSI-LD
/// entities.
#[derive(Parser, Debug)]
#[command(version = short_version(), long_version = long_version(), about = ABOUT, long_about = None)]
#[command(arg_required_else_help = true)]
#[command(disable_help_subcommand = true)]
#[command(styles = THEME)]
pub struct Cli {
    /// Path to a configuration file.
    #[arg(long, short = 'c', global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Show the full detail of every reported problem.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    /// The command to run.
    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    /// How much of each diagnostic the run renders.
    ///
    /// The flag is turned into the domain value here, at the boundary, so no bare boolean travels
    /// any further into the program.
    #[must_use]
    pub const fn verbosity(&self) -> Verbosity {
        if self.verbose { Verbosity::Full } else { Verbosity::Concise }
    }
}

/// The top-level commands the CLI exposes.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Smart Data Models operations.
    #[command(arg_required_else_help = true)]
    Sdm(SdmCommand),

    /// Mapping file operations.
    #[command(arg_required_else_help = true)]
    Mapping(MappingCommand),

    /// JSON Schema operations.
    #[command(arg_required_else_help = true)]
    Schema(SchemaCommand),

    /// Manifest file operations.
    #[command(arg_required_else_help = true)]
    Manifest(Box<ManifestCommand>),

    /// Configuration file operations.
    #[command(arg_required_else_help = true)]
    Config(ConfigCommand),

    /// Map data to a Smart Data Model.
    #[command(arg_required_else_help = true)]
    Map(Box<MapArgs>),

    /// Browse a Smart Data Model schema tree in the terminal.
    Explorer {
        /// Use the light theme instead of the dark theme.
        #[arg(long, default_value_t = false)]
        light: bool,
    },

    /// Author a mapping against a schema in the terminal.
    Wizard {
        /// Use the light theme instead of the dark theme.
        #[arg(long, default_value_t = false)]
        light: bool,
    },

    /// Detect the source format of a data file.
    #[command(arg_required_else_help = true)]
    Profile {
        /// The file to profile.
        file: PathBuf,
    },

    /// Print a diagnostic report to attach when filing a bug.
    Bugreport,

    /// Generate command-line reference documentation.
    #[command(hide = true)]
    Markdown,
}

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use cassiopeia_diagnostic::verbosity::Verbosity;
    use clap::Parser;

    fn parsed(arguments: &[&str]) -> Cli {
        Cli::try_parse_from(arguments).expect("the arguments must parse")
    }

    #[test]
    fn a_run_without_the_flag_is_concise() {
        assert_eq!(parsed(&["cassiopeia", "bugreport"]).verbosity(), Verbosity::Concise);
    }

    #[test]
    fn the_short_flag_selects_the_full_detail() {
        assert_eq!(parsed(&["cassiopeia", "-v", "bugreport"]).verbosity(), Verbosity::Full);
    }

    #[test]
    fn the_long_flag_is_accepted_after_a_subcommand() {
        // The flag is global, so it is accepted wherever a reader would naturally type it.
        assert_eq!(parsed(&["cassiopeia", "bugreport", "--verbose"]).verbosity(), Verbosity::Full);
    }
}

use cassiopeia_cli::config_scaffold::Overwrite;
use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

/// The `config` command group.
#[derive(Parser, Debug)]
pub struct ConfigCommand {
    /// The configuration operation to perform.
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

/// Operations over the installation-wide configuration file.
#[derive(Subcommand, Debug)]
pub enum ConfigSubcommand {
    /// Scaffold a fully-populated default configuration file at the discovered location.
    Generate(ConfigGenArgs),
}

/// Arguments for scaffolding a default configuration file.
#[derive(Args, Debug)]
#[command(next_line_help = true)]
pub struct ConfigGenArgs {
    /// Where to write the file. Defaults to `config.toml` in the XDG config directory (or the
    /// container config root), which is where a subsequent run auto-discovers it.
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Overwrite an existing file instead of refusing to clobber it.
    #[arg(long, action = ArgAction::SetTrue)]
    pub force: bool,
}

impl ConfigGenArgs {
    /// Whether an existing file at the target may be overwritten.
    #[must_use]
    pub const fn overwrite(&self) -> Overwrite {
        Overwrite::from_flag(self.force)
    }
}

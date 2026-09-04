use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// The `mapping` command group.
#[derive(Parser, Debug)]
pub struct MappingCommand {
    /// The mapping operation to perform.
    #[command(subcommand)]
    pub command: MappingSubcommand,
}

/// Operations over JSON5 mapping files.
#[derive(Subcommand, Debug)]
pub enum MappingSubcommand {
    /// Format a JSON5 mapping file.
    #[command(arg_required_else_help = true)]
    Format {
        /// The mapping file to format.
        mapping: PathBuf,

        /// Overwrite the file with the formatted content.
        #[arg(short, long, default_value_t = false)]
        replace: bool,
    },

    /// Convert a JSON5 mapping file to plain JSON.
    #[command(arg_required_else_help = true)]
    Convert {
        /// The JSON5 mapping file to convert.
        mapping: PathBuf,

        /// Output path for the converted file.
        #[arg(short, long)]
        output: PathBuf,
    },
}

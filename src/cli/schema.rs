use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// The `schema` command group.
#[derive(Parser, Debug)]
pub struct SchemaCommand {
    /// The schema operation to perform.
    #[command(subcommand)]
    pub command: SchemaSubcommand,
}

/// Operations over JSON Schema files.
#[derive(Subcommand, Debug)]
pub enum SchemaSubcommand {
    /// Dereference a JSON Schema, inlining its external references.
    #[command(arg_required_else_help = true)]
    Dereference {
        /// The JSON Schema file to dereference.
        schema: PathBuf,
    },
}

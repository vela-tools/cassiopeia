use clap::{Parser, Subcommand};

/// The `sdm` command group.
#[derive(Parser, Debug)]
pub struct SdmCommand {
    /// The Smart Data Models operation to perform.
    #[command(subcommand)]
    pub command: SdmSubcommand,
}

/// Operations over the Smart Data Models schema catalog.
#[derive(Subcommand, Debug)]
pub enum SdmSubcommand {
    /// Download the published schema catalog into the schemas folder.
    Download,

    /// List every schema in the stored catalog.
    List,

    /// Search the stored catalog for schemas whose name matches a query.
    #[command(arg_required_else_help = true)]
    Search {
        /// The query to search for.
        query: String,
    },
}

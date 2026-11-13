use crate::cli::{broker::BrokerArgs, output_format::OutputFormatArgs};
use cassiopeia_common::destination_kind::DestinationKind;
use clap::Args;
use std::path::PathBuf;

/// Arguments describing where and how the `map` command writes its entities.
#[derive(Args, Debug)]
pub struct WriterArgs {
    /// Where to write results: the file system or a Context Broker.
    #[arg(
        short,
        long = "writer",
        value_enum,
        default_value_t = DestinationKind::File,
        help_heading = "Output",
        value_name = "WRITER",
    )]
    pub writer_type: DestinationKind,

    /// Output directory (required when --writer is file).
    #[arg(short, long, required_if_eq("writer_type", "file"), help_heading = "Output", value_name = "DIRECTORY")]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub format: OutputFormatArgs,

    #[command(flatten)]
    pub broker: BrokerArgs,
}

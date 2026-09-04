use crate::cli::broker::BrokerArgs;
use cassiopeia_common::{destination_kind::DestinationKind, file_framing::FileFraming, representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cassiopeia_manifest::output::temporal::TemporalRepresentation;
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

    /// How to handle null values in written entities.
    #[arg(long, value_enum, help_heading = "Output", value_name = "SKIPNULL")]
    pub writer_skip_null: Option<NgsiLdSkipNull>,

    /// Temporal output shape. `series` folds each id's observations into one temporal entity with
    /// instance arrays (ETSI GS CIM 009 v1.9.1 clause 5.2.20); absent writes current-state (one entity
    /// per id, latest per attribute). A general output shape, valid for the file writer and the broker.
    #[arg(long = "temporal-representation", value_name = "REPRESENTATION", help_heading = "Output")]
    pub temporal_representation: Option<TemporalRepresentation>,

    #[command(flatten)]
    pub broker: BrokerArgs,
}

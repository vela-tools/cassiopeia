use cassiopeia_common::{
    attribute_overwrite::AttributeOverwrite,
    broker_header::BrokerHeader,
    broker_operation::BrokerOperationKind,
    tenant::Tenant,
    upsert_mode::UpsertMode,
};
use cassiopeia_manifest::output::user_agent::UserAgent;
use clap::{ArgAction, Args};
use std::str::FromStr;
use url::Url;

/// The Context Broker request flags shared by the `map` and `manifest generate` commands.
///
/// Every field is broker-only, so it conflicts with the file writer's `output` argument (present
/// under that id in both parent commands). Flattening this one struct into both keeps the two
/// command surfaces from drifting apart.
#[derive(Args, Debug)]
pub struct BrokerArgs {
    /// Context Broker URL (required when --writer is context-broker).
    #[arg(
        short = 'u',
        long,
        conflicts_with = "output",
        required_if_eq("writer_type", "context-broker"),
        value_parser = Url::from_str,
        help_heading = "Output",
        value_name = "URL"
    )]
    pub broker_url: Option<Url>,

    /// Which NGSI-LD operation each request performs (context-broker writer only).
    #[arg(
        long,
        value_enum,
        default_value_t = BrokerOperationKind::Upsert,
        conflicts_with = "output",
        help_heading = "Output",
        value_name = "OPERATION"
    )]
    pub broker_operation: BrokerOperationKind,

    /// How a batch upsert reconciles existing entities: replace them or update them in place
    /// (context-broker upsert only).
    #[arg(
        long,
        value_enum,
        default_value_t = UpsertMode::Replace,
        conflicts_with = "output",
        help_heading = "Output",
        value_name = "MODE"
    )]
    pub upsert_mode: UpsertMode,

    /// Whether a batch update overwrites existing attributes or preserves them (context-broker
    /// update only).
    #[arg(
        long,
        value_enum,
        default_value_t = AttributeOverwrite::Overwrite,
        conflicts_with = "output",
        help_heading = "Output",
        value_name = "OVERWRITE"
    )]
    pub attribute_overwrite: AttributeOverwrite,

    /// Spool every entity and push to the broker only after a clean finish (context-broker writer
    /// only).
    #[arg(
        long,
        requires = "broker_url",
        conflicts_with = "output",
        default_value_t = false,
        action = ArgAction::SetTrue,
        help_heading = "Output"
    )]
    pub atomic: bool,

    /// NGSILD-Tenant header value for multi-tenant Context Brokers.
    #[arg(long, help_heading = "Output", value_name = "TENANT")]
    pub tenant: Option<Tenant>,

    /// User-Agent header the broker writer identifies itself with (context-broker writer only).
    /// Overrides the build-time default of `cassiopeia/<version>`.
    #[arg(long, help_heading = "Output", value_name = "USER_AGENT")]
    pub user_agent: Option<UserAgent>,

    /// Extra HTTP header for Context Broker requests, written as `Name: Value`; repeat for several.
    /// Typically carries credentials, e.g. `--header "Authorization: Bearer <token>"`.
    #[arg(long = "header", action = ArgAction::Append, conflicts_with = "output", help_heading = "Output", value_name = "NAME: VALUE")]
    pub headers: Vec<BrokerHeader>,

    /// Send the `@context` via a Link header instead of embedding it in the body.
    #[arg(
        short = 'L',
        long,
        help_heading = "Output",
        conflicts_with = "output",
        default_value_t = false,
        action = ArgAction::SetTrue
    )]
    pub link_header: bool,
}

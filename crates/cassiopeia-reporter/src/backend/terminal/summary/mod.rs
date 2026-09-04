//! The end-of-run report: a counters block plus auto-sized, borderless tables.
//!
//! Column widths are computed by [`tabular`] from the cell contents, so a long label (an entity
//! type, a `Producer -> Consumer` queue name, a broker's own explanation) never shoves the following
//! columns out of alignment. The layout adapts instead of relying on hand-tuned width constants.
//!
//! One concern per module: [`run_summary`] the whole report, [`counters_block`] the run totals,
//! [`stages_table`] per-stage throughput, [`channels_table`] per-channel queue depth,
//! [`reasons_table`] why the run's failures happened, [`table_layout`] the shared header styling,
//! and [`thousands`] the digit-group separator.

pub(crate) mod channels_table;
pub(crate) mod counters_block;
pub(crate) mod reasons_table;
pub(crate) mod run_summary;
pub(crate) mod stages_table;
pub(crate) mod table_layout;
pub(crate) mod thousands;

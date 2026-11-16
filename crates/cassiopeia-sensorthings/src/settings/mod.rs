//! The `sensorthings` block of a remote input: which Datastreams and `MultiDatastreams` are read,
//! how, how far one cycle goes, whether each reading is read with the feature it was made at,
//! whether each Thing is read with the places it has been, and which Things a latest read lists by
//! themselves.

pub mod collect_limits;
pub mod entity_filter;
pub mod features_of_interest;
pub mod filter_setting;
pub mod historical_locations;
pub mod incremental_read;
pub mod multi_datastreams;
pub mod page_size;
pub mod read_mode;
pub mod request_delay;
pub mod sensorthings_settings;
pub mod things;
pub mod time_range;

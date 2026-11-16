//! The `sensorthings` block of a remote input: which Datastreams and `MultiDatastreams` are read,
//! how, how far one cycle goes, whether each reading is read with the feature it was made at, and
//! whether each Thing is read with the places it has been.

pub mod collect_limits;
pub mod features_of_interest;
pub mod historical_locations;
pub mod incremental_read;
pub mod multi_datastreams;
pub mod page_size;
pub mod read_mode;
pub mod request_delay;
pub mod sensorthings_settings;
pub mod stream_filter;
pub mod time_range;

//! The `sensorthings` block of a remote input: which data streams are read, how, how far one cycle
//! goes, and whether each reading is read with the feature it was made at.

pub mod collect_limits;
pub mod datastream_filter;
pub mod features_of_interest;
pub mod incremental_read;
pub mod page_size;
pub mod read_mode;
pub mod request_delay;
pub mod sensorthings_settings;
pub mod time_range;

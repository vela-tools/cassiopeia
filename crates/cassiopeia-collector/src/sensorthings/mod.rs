//! Collecting a SensorThings API server: the requests of one cycle, followed page by page, written
//! into a snapshot file the profiler and the ingestor read as they would a local one, and the
//! metadata discovery a command reads the same way. Datastreams and the `MultiDatastreams` an input
//! selects are read through the same traversal, paging, limits and pacing. An incremental read
//! resumes from the positions its cursor committed and leaves the update of the cycle for the
//! pipeline to commit, and a run checks once whether its server holds `MultiDatastreams` an input
//! reads none of.

pub mod collection_listing;
pub mod collector;
pub mod cursor_cycle;
pub mod cursor_update_slot;
pub mod datastreams_missing;
mod incremental_read;
pub mod limit_reached;
pub mod metadata_discovery;
pub mod multi_datastreams_unread;
mod observation_budget;
mod observation_read;
mod page_reader;
pub mod range_empty;
mod range_read;
mod request_pacing;
mod snapshot_failure;
mod stream_discovery;
#[cfg(test)]
mod test_collection;
pub mod traversal;
pub mod unread_check;

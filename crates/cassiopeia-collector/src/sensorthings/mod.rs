//! Collecting a SensorThings API server: the requests of one cycle, followed page by page, written
//! into a snapshot file the profiler and the ingestor read as they would a local one, and the
//! metadata discovery a command reads the same way. An incremental read resumes from the positions
//! its cursor committed and leaves the update of the cycle for the pipeline to commit.

pub mod collector;
pub mod cursor_cycle;
pub mod cursor_update_slot;
pub mod datastreams_missing;
mod incremental_read;
pub mod limit_reached;
pub mod metadata_discovery;
mod observation_budget;
mod observation_read;
mod page_reader;
pub mod range_empty;
mod range_read;
mod request_pacing;
mod snapshot_failure;
#[cfg(test)]
mod test_collection;
pub mod traversal;

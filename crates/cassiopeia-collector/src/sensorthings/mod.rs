//! Collecting a SensorThings API server: the requests of one cycle, followed page by page, written
//! into a snapshot file the profiler and the ingestor read as they would a local one, and the
//! metadata discovery a command reads the same way.

pub mod collector;
pub mod limit_reached;
pub mod metadata_discovery;
mod observation_budget;
mod page_reader;
pub mod range_empty;
mod range_read;
mod request_pacing;
mod snapshot_failure;
#[cfg(test)]
mod test_collection;
pub mod traversal;

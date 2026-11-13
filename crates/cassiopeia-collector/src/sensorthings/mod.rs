//! Collecting a SensorThings API server: the requests of one cycle, followed page by page, written
//! into a snapshot file the profiler and the ingestor read as they would a local one.

pub mod collector;
pub mod limit_reached;
mod page_reader;
mod request_pacing;
mod traversal;

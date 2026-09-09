//! Collection stage for the Cassiopeia pipeline.
//!
//! The collector is the transport head of the pipeline: it resolves each configured
//! [`Input`](cassiopeia_common::input::Input) to a local file (using a local path in place or
//! downloading a remote URL into a temporary directory) and emits one
//! [`CollectedPayload`](cassiopeia_ir::payload::CollectedPayload) per source for the profiler to
//! read.

pub mod collector;
pub mod downloader;
pub mod error;
pub mod file_extension;
pub mod generic;
pub mod source;

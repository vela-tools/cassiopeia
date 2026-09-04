//! Profiling stage for the Cassiopeia pipeline.
//!
//! The profiler sits between the collector and the ingestor bank. It reads
//! [`CollectedPayload`](cassiopeia_ir::payload::CollectedPayload) values, determines each one's
//! source format, and routes it (wrapped as a
//! [`ProfiledPayload`](cassiopeia_ir::payload::ProfiledPayload)) to the ingestor channel for that
//! format. Two strategies are provided: [`AutoProfiler`](auto_profiler::AutoProfiler) detects the
//! format from the content via [`cassiopeia_data_profiler`], and
//! [`PassthroughProfiler`](passthrough_profiler::PassthroughProfiler) takes the format the caller
//! already declared.

pub mod auto_profiler;
pub mod error;
pub mod passthrough_profiler;
pub mod profiler;
mod routing;

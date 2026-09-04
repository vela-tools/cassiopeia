use cassiopeia_common::format::DataFormat;
use cassiopeia_data_profiler::error::DataProfilerError;
use cassiopeia_ir::payload_origin::PayloadOrigin;
use thiserror::Error;

/// A failure raised while profiling a payload or routing it to an ingestor.
#[derive(Debug, Error)]
pub enum ProfilerError {
    /// The format could not be determined from the payload's content.
    #[error("The payload's data format could not be determined")]
    Profiling(#[from] DataProfilerError),

    /// The ingestor channel for a profiled payload's format was closed, which means the pipeline is
    /// shutting down.
    #[error("downstream channel closed")]
    ChannelClosed,

    /// No ingestor is registered for the detected format.
    #[error("No ingestor is registered for format {format}, detected on {origin}")]
    NoRoute {
        /// The format that had no registered route.
        format: DataFormat,
        /// The payload whose format could not be routed.
        origin: PayloadOrigin,
    },
}

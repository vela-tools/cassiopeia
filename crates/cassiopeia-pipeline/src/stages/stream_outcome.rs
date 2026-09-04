/// Whether a pump stage's input stream ended cleanly or after a failure.
///
/// The runner reports this to [`PumpProcessor::finalize`](crate::stages::pump::PumpProcessor::finalize)
/// so a stage that must distinguish a clean end from a mid-stream failure (the writer, when it
/// stages output atomically) can act on it. Streaming stages ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamOutcome {
    /// The stream ended with no forwarded error and no cancellation.
    Clean,
    /// The stream forwarded at least one error, or the run was cancelled.
    Failed,
}

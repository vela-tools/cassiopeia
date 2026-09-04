/// How a run reached the writer's [`finalize`](crate::writer::Writer::finalize).
///
/// A streaming writer has already emitted its output by the time finalize runs, so it ignores the
/// outcome; a staging writer uses it to decide whether to commit its spool or discard it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The pipeline finished cleanly: buffered output should be committed.
    Committed,
    /// The pipeline failed or was cancelled: buffered output should be discarded.
    Aborted,
}

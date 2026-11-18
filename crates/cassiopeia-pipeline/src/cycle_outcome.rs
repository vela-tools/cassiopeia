/// How a cycle that did not fail ended.
///
/// A failed cycle is the error its run returns; a stop is no failure, so the failure policy, the
/// retries and the exit status never see it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CycleOutcome {
    /// The cycle ran to its end, and the cursors of its incremental inputs moved past what it read.
    Completed,
    /// The run's stop cut the cycle short, wherever its stages were, so its cursors stay where they
    /// were and the next run reads what it read again.
    Stopped,
}

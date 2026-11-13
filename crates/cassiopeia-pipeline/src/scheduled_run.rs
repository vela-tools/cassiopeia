use cassiopeia_common::{cycle_start::CycleStart, signal::MetaSignal};

/// One scheduled run, as every attempt at it runs.
///
/// The instant the run started is taken once, before its first attempt, so a retried attempt builds
/// the same requests and records the same collection time as the attempt it retries.
pub(crate) struct ScheduledRun {
    /// The schedule context forwarded to the resolver.
    pub(crate) meta: MetaSignal,
    /// The instant the run started.
    pub(crate) start: CycleStart,
}

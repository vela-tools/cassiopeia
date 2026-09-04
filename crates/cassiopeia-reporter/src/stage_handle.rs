//! A running stage's own handle to the backend state that draws it.

use cassiopeia_common::telemetry::stage_metrics::RateReader;

/// The backend state one running stage draws through, resolved once when the stage is entered.
///
/// A stage worker holds exactly one handle for as long as it runs and emits every progress event
/// through it, so a hot path never looks its stage up in the backend's id-keyed registry. That
/// lookup is a process-wide lock shared by every stage thread, and at one acquisition per record it
/// is the single most contended thing in the pipeline; resolving it once per stage removes it.
///
/// Counts arrive in bulk: one call per batch, not one per item, so every method takes the number
/// of items it accounts for. Backends that draw nothing implement this as [`SilentStageHandle`].
pub trait StageHandle: Send + Sync {
    /// Advances the stage's processed count by `count` items.
    fn inc_by(&self, count: u64);

    /// Declares the stage's total length, switching its bar to determinate progress.
    fn set_length(&self, length: u64);

    /// Marks the stage as having warned, without changing its visible warning count.
    fn warn(&self);

    /// Adds `count` to the stage's visible warning count and marks it as having warned.
    fn warn_inc_by(&self, count: u64);

    /// Wires the stage's live throughput readout to the run's aggregate rate for that stage.
    ///
    /// Called once, when a worker attaches its telemetry to the guard, so the readout reports the
    /// same measured rate the stage records into rather than counting a second time.
    fn attach_rate(&self, reader: RateReader);

    /// Finishes the stage. Called exactly once, when the stage's guard is dropped.
    fn finish(&self);
}

/// A stage handle that draws nothing.
///
/// Backends with no visible stage display (the no-op reporter, and any consumer that only wants the
/// telemetry half of a stage guard) hand this out so a stage's progress calls compile to nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct SilentStageHandle;

impl SilentStageHandle {
    /// Builds a handle that discards every progress event.
    #[must_use]
    pub const fn new() -> SilentStageHandle {
        SilentStageHandle
    }
}

impl StageHandle for SilentStageHandle {
    fn inc_by(&self, _count: u64) {}
    fn set_length(&self, _length: u64) {}
    fn warn(&self) {}
    fn warn_inc_by(&self, _count: u64) {}
    fn attach_rate(&self, _reader: RateReader) {}
    fn finish(&self) {}
}

#[cfg(test)]
mod tests {
    use crate::stage_handle::{SilentStageHandle, StageHandle};

    #[test]
    fn a_silent_handle_accepts_every_event_without_panicking() {
        let handle = SilentStageHandle::new();

        handle.inc_by(1_000);
        handle.set_length(10);
        handle.warn();
        handle.warn_inc_by(3);
        handle.finish();
    }
}

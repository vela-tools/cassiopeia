//! RAII guard for progress stage management.

use crate::stage_handle::{SilentStageHandle, StageHandle};
use cassiopeia_common::telemetry::{
    component::StageComponent,
    stage_metrics::{ServiceSpan, StageTelemetry},
};
use std::time::Duration;

/// RAII guard for a progress stage, and the single handle a stage records every event through.
///
/// A worker enters a stage once and holds one guard. The guard owns the backend's
/// [`StageHandle`] for that stage, resolved when the stage was entered, so no progress call
/// re-resolves the stage through the reporter. When the guard also carries a [`StageTelemetry`]
/// (attached via [`StageGuard::with_telemetry`]), each progress event updates both the visible
/// progress display and the run telemetry in a single call, so a stage never records the same
/// completion twice. Dropping the guard finishes the stage and drops the telemetry handle, which
/// closes the worker's lifecycle.
///
/// The counting methods take a count because stages hand work over in batches: a stage reports one
/// batch's worth of progress with one call rather than one call per item.
pub struct StageGuard {
    handle: Box<dyn StageHandle>,
    telemetry: Option<StageTelemetry>,
}

impl StageGuard {
    /// Creates a stage guard over the backend handle for one entered stage.
    #[must_use]
    pub fn new(handle: Box<dyn StageHandle>) -> StageGuard {
        StageGuard { handle, telemetry: None }
    }

    /// Creates a stage guard that draws nothing, for a consumer that wants only its telemetry half.
    #[must_use]
    pub fn silent() -> StageGuard {
        StageGuard::new(Box::new(SilentStageHandle::new()))
    }

    /// Attaches this worker's telemetry handle, wiring the stage's rate readout to the aggregate.
    ///
    /// After this, the counting and timing methods record into the run telemetry as well as the
    /// visible progress display.
    #[must_use]
    pub fn with_telemetry(mut self, telemetry: StageTelemetry) -> StageGuard {
        self.handle.attach_rate(telemetry.rate_reader());
        self.telemetry = Some(telemetry);
        self
    }

    /// Increments progress for this stage and records one completed item.
    pub fn inc(&self) {
        self.inc_by(1);
    }

    /// Advances progress by `count` and records `count` completed items.
    ///
    /// This is the batch form of [`StageGuard::inc`]: one call accounts for a whole batch, so the
    /// backend's display state and the run telemetry are each touched once per batch.
    pub fn inc_by(&self, count: u64) {
        self.handle.inc_by(count);
        if let Some(telemetry) = &self.telemetry {
            telemetry.completed(count);
        }
    }

    /// Sets the total length for this stage.
    pub fn set_length(&self, length: u64) {
        self.handle.set_length(length);
    }

    /// Marks this stage as having a warning and records one warned item.
    pub fn warn(&self) {
        self.handle.warn();
        if let Some(telemetry) = &self.telemetry {
            telemetry.warning(1);
        }
    }

    /// Increments the visible warning count, marks this stage as warned, and records one warned item.
    pub fn warn_inc(&self) {
        self.warn_inc_by(1);
    }

    /// Adds `count` to the visible warning count, marks this stage as warned, and records `count`
    /// warned items.
    pub fn warn_inc_by(&self, count: u64) {
        self.handle.warn_inc_by(count);
        if let Some(telemetry) = &self.telemetry {
            telemetry.warning(count);
        }
    }

    /// Records `count` completed items in telemetry without advancing the visible progress display.
    ///
    /// Used where an item completes the stage's work but is not counted as processed on the bar: a
    /// validator entity that is forwarded but recorded as a warning rather than a pass.
    pub fn complete(&self, count: u64) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.completed(count);
        }
    }

    /// Records `count` failed items for this stage.
    pub fn fail(&self, count: u64) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.failed(count);
        }
    }

    /// Records `count` items received by this stage.
    pub fn received(&self, count: u64) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.received(count);
        }
    }

    /// Records active service time for this stage.
    pub fn add_service_time(&self, duration: Duration) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.add_service_time(duration);
        }
    }

    /// Opens a service span that records this stage's wall and CPU time when it is dropped, or `None`
    /// when the guard carries no telemetry. Drop it explicitly, to close the span before later
    /// output-wait work, at the point service ends.
    #[must_use]
    pub fn service_span(&self) -> Option<ServiceSpan<'_>> {
        self.telemetry.as_ref().map(StageTelemetry::service_span)
    }

    /// Records time this stage spent waiting on its input.
    pub fn add_input_wait(&self, duration: Duration) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.add_input_wait(duration);
        }
    }

    /// Records time this stage spent waiting to hand work downstream.
    pub fn add_output_wait(&self, duration: Duration) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.add_output_wait(duration);
        }
    }

    /// Records time spent in a distinct sub-phase of this stage's work.
    pub fn component_time(&self, component: StageComponent, duration: Duration) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.component_time(component, duration);
        }
    }

    /// Times `operation` and records it as service time.
    pub fn measure_service<T>(&self, operation: impl FnOnce() -> T) -> T {
        match &self.telemetry {
            Some(telemetry) => telemetry.measure_service(operation),
            None => operation(),
        }
    }

    /// Times `operation` and records it as input-wait time.
    pub fn measure_input_wait<T>(&self, operation: impl FnOnce() -> T) -> T {
        match &self.telemetry {
            Some(telemetry) => telemetry.measure_input_wait(operation),
            None => operation(),
        }
    }

    /// Times `operation` and records it as output-wait time.
    pub fn measure_output_wait<T>(&self, operation: impl FnOnce() -> T) -> T {
        match &self.telemetry {
            Some(telemetry) => telemetry.measure_output_wait(operation),
            None => operation(),
        }
    }
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        self.handle.finish();
    }
}

#[cfg(test)]
mod tests {
    use crate::{guard::StageGuard, stage_handle::StageHandle};
    use cassiopeia_common::{
        stage::Stage,
        telemetry::{run::RunTelemetry, stage_metrics::RateReader},
    };
    use parking_lot::Mutex;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    /// A handle that records every event it is handed, so a test can read back exactly what the
    /// guard forwarded to the backend.
    #[derive(Default)]
    struct RecordingHandle {
        incremented: AtomicU64,
        inc_calls: AtomicU64,
        warned: AtomicU64,
        finished: AtomicU64,
        lengths: Mutex<Vec<u64>>,
    }

    impl StageHandle for RecordingHandle {
        fn inc_by(&self, count: u64) {
            self.incremented.fetch_add(count, Ordering::SeqCst);
            self.inc_calls.fetch_add(1, Ordering::SeqCst);
        }
        fn set_length(&self, length: u64) {
            self.lengths.lock().push(length);
        }
        fn warn(&self) {
            self.warned.fetch_add(1, Ordering::SeqCst);
        }
        fn warn_inc_by(&self, count: u64) {
            self.warned.fetch_add(count, Ordering::SeqCst);
        }
        fn attach_rate(&self, _reader: RateReader) {}
        fn finish(&self) {
            self.finished.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A guard over a shared recording handle, so the test keeps a reader after the guard is built.
    fn guard() -> (StageGuard, Arc<RecordingHandle>) {
        let handle = Arc::new(RecordingHandle::default());
        (StageGuard::new(Box::new(SharedHandle(Arc::clone(&handle)))), handle)
    }

    /// Forwards to a shared recording handle, so a test can hold a reader while the guard owns one.
    struct SharedHandle(Arc<RecordingHandle>);

    impl StageHandle for SharedHandle {
        fn inc_by(&self, count: u64) {
            self.0.inc_by(count);
        }
        fn set_length(&self, length: u64) {
            self.0.set_length(length);
        }
        fn warn(&self) {
            self.0.warn();
        }
        fn warn_inc_by(&self, count: u64) {
            self.0.warn_inc_by(count);
        }
        fn attach_rate(&self, reader: RateReader) {
            self.0.attach_rate(reader);
        }
        fn finish(&self) {
            self.0.finish();
        }
    }

    #[test]
    fn dropping_the_guard_finishes_its_stage() {
        let (guard, handle) = guard();
        drop(guard);

        assert_eq!(handle.finished.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn inc_forwards_a_single_item_to_the_handle() {
        let (guard, handle) = guard();
        guard.inc();

        assert_eq!(handle.incremented.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn inc_by_bumps_the_handle_and_the_telemetry_exactly_once_each() {
        let telemetry = RunTelemetry::new();
        let (guard, handle) = guard();
        let guard = guard.with_telemetry(telemetry.start_stage(Stage::Writer));

        guard.inc_by(10_000);
        drop(guard);

        // One call carrying the whole batch, not ten thousand calls.
        assert_eq!(handle.inc_calls.load(Ordering::SeqCst), 1);
        assert_eq!(handle.incremented.load(Ordering::SeqCst), 10_000);
        let snapshot = telemetry.snapshots().into_iter().next().expect("one writer stage");
        assert_eq!(snapshot.completed, 10_000);
    }

    #[test]
    fn warn_inc_by_counts_every_warned_item_once_in_both_places() {
        let telemetry = RunTelemetry::new();
        let (guard, handle) = guard();
        let guard = guard.with_telemetry(telemetry.start_stage(Stage::Validator));

        guard.warn_inc_by(7);
        drop(guard);

        assert_eq!(handle.warned.load(Ordering::SeqCst), 7);
        let snapshot = telemetry.snapshots().into_iter().next().expect("one validator stage");
        assert_eq!(snapshot.warnings, 7);
    }

    #[test]
    fn a_silent_guard_records_telemetry_without_a_backend() {
        let telemetry = RunTelemetry::new();
        let guard = StageGuard::silent().with_telemetry(telemetry.start_stage(Stage::Extractor));

        guard.inc_by(3);
        drop(guard);

        let snapshot = telemetry.snapshots().into_iter().next().expect("one extractor stage");
        assert_eq!(snapshot.completed, 3);
    }
}

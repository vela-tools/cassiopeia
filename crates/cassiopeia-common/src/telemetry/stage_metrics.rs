//! Per-stage measurement state, the worker handle that records into it, and its snapshot.

use crate::{
    stage::Stage,
    telemetry::{
        component::StageComponent,
        nanos::{from_nanos, to_nanos},
        rate::{RateSnapshot, RateTracker},
    },
};
use cpu_time::ThreadTime;
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A logical stage's immutable metrics at a point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct StageSnapshot {
    pub stage: Stage,
    /// `None` identifies the logical aggregate; instances carry their instance name.
    pub instance: Option<String>,
    pub active_workers: usize,
    pub worker_count: usize,
    pub received: u64,
    pub completed: u64,
    pub failed: u64,
    pub warnings: u64,
    pub service_time: Duration,
    /// CPU time consumed inside the stage's synchronous service span, as opposed to the wall time it
    /// occupied. It is only measured for work routed through [`StageTelemetry::measure_service`], so a
    /// stage that never brackets synchronous compute reads zero here.
    pub service_cpu: Duration,
    pub input_wait: Duration,
    pub output_wait: Duration,
    pub wall_time: Duration,
    pub components: BTreeMap<StageComponent, Duration>,
    pub rates: RateSnapshot,
}

/// The lifecycle bookkeeping that lets a stage's wall time span all its workers.
#[derive(Debug, Default)]
struct Lifecycle {
    active: usize,
    first_started: Option<Instant>,
    last_finished: Option<Instant>,
}

/// The concurrent measurement state for one logical stage (or one worker instance of it).
#[derive(Debug, Default)]
pub(crate) struct StageState {
    received: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
    warnings: AtomicU64,
    service_ns: AtomicU64,
    service_cpu_ns: AtomicU64,
    input_wait_ns: AtomicU64,
    output_wait_ns: AtomicU64,
    /// When this stage first received work, as nanoseconds since the run started, or zero while it
    /// has received nothing. It is the average-throughput denominator, seeded once by the first
    /// [`StageTelemetry::received`] call; keeping it here rather than in the rate tracker is what
    /// lets the receive path skip the tracker's lock and its clock read entirely.
    first_input_ns: AtomicU64,
    components: Mutex<BTreeMap<StageComponent, u64>>,
    lifecycle: Mutex<Lifecycle>,
    rates: Mutex<RateTracker>,
}

impl StageState {
    /// Marks one worker as started, seeding the stage's first-started instant.
    pub(crate) fn begin(&self, started: Instant) {
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.active += 1;
        lifecycle.first_started.get_or_insert(started);
    }

    /// Marks one worker as finished.
    fn finish(&self) {
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.active = lifecycle.active.saturating_sub(1);
        lifecycle.last_finished = Some(Instant::now());
    }

    /// Returns when this stage first received work, or `None` while it has received nothing.
    fn first_input(&self) -> Option<Duration> {
        match self.first_input_ns.load(Ordering::Relaxed) {
            0 => None,
            nanos => Some(from_nanos(nanos)),
        }
    }

    /// Seeds the first-input instant the first time work arrives, doing nothing afterwards.
    ///
    /// The load short-circuits every call after the first, so the run-elapsed clock is read exactly
    /// once per stage state rather than once per received batch.
    fn seed_first_input(&self, run_started: Instant) {
        if self.first_input_ns.load(Ordering::Relaxed) == 0 {
            // Zero means "no input yet", so a stage that receives within the run's first nanosecond
            // rounds up to one rather than reading as unseeded.
            let elapsed = to_nanos(run_started.elapsed()).max(1);
            let _ = self.first_input_ns.compare_exchange(0, elapsed, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
}

/// A cheap, cloneable read handle over one stage's aggregate rolling rate.
///
/// The reporter holds one of these to render a stage bar's live throughput, so the bar reads the same
/// aggregate the stage records into rather than maintaining a second, parallel rate tracker.
#[derive(Clone)]
pub struct RateReader {
    aggregate: Arc<StageState>,
    run_started: Instant,
}

impl RateReader {
    /// Returns the current rolling live throughput in items per second.
    #[must_use]
    pub fn live_throughput(&self) -> f64 {
        self.aggregate
            .rates
            .lock()
            .snapshot_at(self.run_started.elapsed(), Duration::ZERO, self.aggregate.first_input())
            .live_throughput
    }
}

/// An open service span: an RAII guard that records a stage's synchronous work when it is dropped.
///
/// [`StageTelemetry::service_span`] opens one at the start of a unit of work. Dropping it (at the
/// end of the enclosing scope, or explicitly with `drop` to close the span before later work such as
/// an output-wait send) records the wall time it covered and, when the platform reports it, the CPU
/// time consumed during it. It captures the per-thread CPU clock, so it is not `Send` and must be
/// dropped on the thread that opened it, with no intervening `.await`.
pub struct ServiceSpan<'a> {
    telemetry: &'a StageTelemetry,
    started: Instant,
    cpu_started: Option<ThreadTime>,
}

impl Drop for ServiceSpan<'_> {
    fn drop(&mut self) {
        let cpu = self.cpu_started.and_then(|started| started.try_elapsed().ok());
        self.telemetry.add_service_time(self.started.elapsed());
        if let Some(cpu) = cpu {
            self.telemetry.add_service_cpu(cpu);
        }
    }
}

/// A cheap handle owned by one worker. Each handle has independent lifecycle state while updating the
/// logical aggregate, so dropping one worker cannot finish or reset another worker.
#[derive(Debug)]
pub struct StageTelemetry {
    aggregate: Arc<StageState>,
    instance: Arc<StageState>,
    run_started: Instant,
}

impl StageTelemetry {
    pub(crate) const fn new(aggregate: Arc<StageState>, instance: Arc<StageState>, run_started: Instant) -> StageTelemetry {
        StageTelemetry {
            aggregate,
            instance,
            run_started,
        }
    }

    /// Returns a read handle over this stage's aggregate rolling rate.
    #[must_use]
    pub fn rate_reader(&self) -> RateReader {
        RateReader {
            aggregate: Arc::clone(&self.aggregate),
            run_started: self.run_started,
        }
    }

    pub fn received(&self, count: u64) {
        self.record_received(&self.aggregate, count);
        self.record_received(&self.instance, count);
    }
    pub fn completed(&self, count: u64) {
        self.record_completed(&self.aggregate, count);
        self.record_completed(&self.instance, count);
    }
    pub fn failed(&self, count: u64) {
        self.aggregate.failed.fetch_add(count, Ordering::Relaxed);
        self.instance.failed.fetch_add(count, Ordering::Relaxed);
    }
    pub fn warning(&self, count: u64) {
        self.aggregate.warnings.fetch_add(count, Ordering::Relaxed);
        self.instance.warnings.fetch_add(count, Ordering::Relaxed);
    }
    pub fn add_service_time(&self, duration: Duration) {
        Self::add_time(&self.aggregate.service_ns, &self.instance.service_ns, duration);
    }
    pub fn add_service_cpu(&self, duration: Duration) {
        Self::add_time(&self.aggregate.service_cpu_ns, &self.instance.service_cpu_ns, duration);
    }
    /// Opens a [`ServiceSpan`] that records this stage's wall and CPU time when it is dropped.
    ///
    /// Every stage brackets its synchronous per-item work with a span so both the wall time and the
    /// CPU time of that work are recorded uniformly, without each stage duplicating the clock reads.
    #[must_use]
    pub fn service_span(&self) -> ServiceSpan<'_> {
        ServiceSpan {
            telemetry: self,
            started: Instant::now(),
            cpu_started: ThreadTime::try_now().ok(),
        }
    }
    pub fn add_input_wait(&self, duration: Duration) {
        Self::add_time(&self.aggregate.input_wait_ns, &self.instance.input_wait_ns, duration);
    }
    pub fn add_output_wait(&self, duration: Duration) {
        Self::add_time(&self.aggregate.output_wait_ns, &self.instance.output_wait_ns, duration);
    }
    pub fn component_time(&self, component: StageComponent, duration: Duration) {
        for state in [&self.aggregate, &self.instance] {
            let mut components = state.components.lock();
            let entry = components.entry(component).or_default();
            *entry = entry.saturating_add(to_nanos(duration));
        }
    }
    pub fn measure_service<T>(&self, operation: impl FnOnce() -> T) -> T {
        let _span = self.service_span();
        operation()
    }
    pub fn measure_input_wait<T>(&self, operation: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = operation();
        self.add_input_wait(started.elapsed());
        result
    }
    pub fn measure_output_wait<T>(&self, operation: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = operation();
        self.add_output_wait(started.elapsed());
        result
    }
    fn record_received(&self, state: &StageState, count: u64) {
        state.received.fetch_add(count, Ordering::Relaxed);
        state.seed_first_input(self.run_started);
    }
    fn record_completed(&self, state: &StageState, count: u64) {
        state.completed.fetch_add(count, Ordering::Relaxed);
        state.rates.lock().record_completed_at(self.run_started.elapsed(), count);
    }
    fn add_time(aggregate: &AtomicU64, instance: &AtomicU64, duration: Duration) {
        let nanos = to_nanos(duration);
        aggregate.fetch_add(nanos, Ordering::Relaxed);
        instance.fetch_add(nanos, Ordering::Relaxed);
    }
}

impl Drop for StageTelemetry {
    fn drop(&mut self) {
        self.aggregate.finish();
        self.instance.finish();
    }
}

/// Builds an immutable snapshot of one stage state.
pub(crate) fn snapshot(
    stage: Stage,
    instance: Option<String>,
    stage_state: &StageState,
    worker_count: usize,
    now: Instant,
    run_elapsed: Duration,
) -> StageSnapshot {
    let lifecycle = stage_state.lifecycle.lock();
    let wall_time = match (lifecycle.first_started, lifecycle.last_finished, lifecycle.active) {
        (Some(first), Some(last), 0) => last.duration_since(first),
        (Some(first), _, _) => now.duration_since(first),
        _ => Duration::ZERO,
    };
    let active_workers = lifecycle.active;
    drop(lifecycle);
    let service_time = from_nanos(stage_state.service_ns.load(Ordering::Relaxed));
    let service_cpu = from_nanos(stage_state.service_cpu_ns.load(Ordering::Relaxed));
    let rates = stage_state.rates.lock().snapshot_at(run_elapsed, service_time, stage_state.first_input());
    StageSnapshot {
        stage,
        instance,
        active_workers,
        worker_count,
        received: stage_state.received.load(Ordering::Relaxed),
        completed: stage_state.completed.load(Ordering::Relaxed),
        failed: stage_state.failed.load(Ordering::Relaxed),
        warnings: stage_state.warnings.load(Ordering::Relaxed),
        service_time,
        service_cpu,
        input_wait: from_nanos(stage_state.input_wait_ns.load(Ordering::Relaxed)),
        output_wait: from_nanos(stage_state.output_wait_ns.load(Ordering::Relaxed)),
        wall_time,
        components: stage_state
            .components
            .lock()
            .iter()
            .map(|(component, nanos)| (*component, from_nanos(*nanos)))
            .collect(),
        rates,
    }
}

//! The shared telemetry registry for one pipeline run and its immutable summary.

use crate::{
    channel::ChannelMetrics,
    stage::Stage,
    telemetry::{
        channel_metrics::ChannelSnapshot,
        cpu::{CpuSnapshot, CpuUsage},
        memory::{MemorySnapshot, MemoryUsage},
        run_counters::{AtomicRunCounters, RunCounters},
        stage_metrics::{StageSnapshot, StageState, StageTelemetry, snapshot},
    },
};
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// A structured, immutable run summary.
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetrySnapshot {
    pub elapsed: Duration,
    pub counters: RunCounters,
    pub memory: MemorySnapshot,
    pub cpu: CpuSnapshot,
    pub stages: Vec<StageSnapshot>,
    pub channels: Vec<ChannelSnapshot>,
}

/// One logical stage's aggregate state plus its per-worker instances.
#[derive(Debug, Default)]
struct StageEntry {
    aggregate: Arc<StageState>,
    instances: BTreeMap<String, Arc<StageState>>,
}

/// Shared telemetry registry for one pipeline run.
#[derive(Debug)]
pub struct RunTelemetry {
    started: Instant,
    stages: Mutex<BTreeMap<Stage, StageEntry>>,
    channels: Mutex<Vec<Arc<ChannelMetrics>>>,
    counters: AtomicRunCounters,
    next_instance: AtomicUsize,
    memory: MemoryUsage,
    cpu: CpuUsage,
}

impl Default for RunTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl RunTelemetry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            stages: Mutex::new(BTreeMap::new()),
            channels: Mutex::new(Vec::new()),
            counters: AtomicRunCounters::default(),
            next_instance: AtomicUsize::new(0),
            memory: MemoryUsage::default(),
            cpu: CpuUsage::new(),
        }
    }

    /// Records one process resident-memory sample, in bytes, taken by the run's memory sampler.
    pub fn record_memory(&self, resident_bytes: u64) {
        self.memory.record(resident_bytes);
    }

    /// Starts a uniquely named worker instance under a logical stage.
    pub fn start_stage(&self, stage: Stage) -> StageTelemetry {
        let number = self.next_instance.fetch_add(1, Ordering::Relaxed);
        self.start_stage_instance(stage, format!("{stage}/lane-{number}"))
    }

    /// Starts a named worker instance under a logical stage.
    pub fn start_stage_instance(&self, stage: Stage, instance: String) -> StageTelemetry {
        let started = Instant::now();
        let mut stages = self.stages.lock();
        let entry = stages.entry(stage).or_default();
        let instance = Arc::clone(entry.instances.entry(instance).or_insert_with(|| Arc::new(StageState::default())));
        entry.aggregate.begin(started);
        instance.begin(started);
        StageTelemetry::new(Arc::clone(&entry.aggregate), instance, self.started)
    }

    pub(crate) fn register_channel(&self, channel: Arc<ChannelMetrics>) {
        self.channels.lock().push(channel);
    }

    pub fn add_input_records(&self, count: u64) {
        self.counters.input_records.fetch_add(count, Ordering::Relaxed);
    }
    pub fn add_fragments_created(&self, count: u64) {
        self.counters.fragments_created.fetch_add(count, Ordering::Relaxed);
    }
    pub fn set_unique_entities(&self, count: u64) {
        self.counters.unique_entities.store(count, Ordering::Relaxed);
    }
    pub fn add_entities_written(&self, count: u64) {
        self.counters.entities_written.fetch_add(count, Ordering::Relaxed);
    }
    pub fn add_errors(&self, count: u64) {
        self.counters.errors.fetch_add(count, Ordering::Relaxed);
    }
    pub fn add_warnings(&self, count: u64) {
        self.counters.warnings.fetch_add(count, Ordering::Relaxed);
    }
    pub fn add_bytes_read(&self, count: u64) {
        self.counters.bytes_read.fetch_add(count, Ordering::Relaxed);
    }
    pub fn add_bytes_written(&self, count: u64) {
        self.counters.bytes_written.fetch_add(count, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshots(&self) -> Vec<StageSnapshot> {
        let now = Instant::now();
        let run_elapsed = self.started.elapsed();
        self.stages
            .lock()
            .iter()
            .map(|(stage, entry)| snapshot(*stage, None, &entry.aggregate, entry.instances.len(), now, run_elapsed))
            .collect()
    }

    #[must_use]
    pub fn instance_snapshots(&self) -> Vec<StageSnapshot> {
        let now = Instant::now();
        let run_elapsed = self.started.elapsed();
        self.stages
            .lock()
            .iter()
            .flat_map(|(stage, entry)| {
                entry
                    .instances
                    .iter()
                    .map(move |(instance, state)| snapshot(*stage, Some(instance.clone()), state, 1, now, run_elapsed))
            })
            .collect()
    }

    #[must_use]
    pub fn channel_snapshots(&self) -> Vec<ChannelSnapshot> {
        self.channels.lock().iter().map(|channel| channel.snapshot()).collect()
    }

    #[must_use]
    pub fn snapshot(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            elapsed: self.elapsed(),
            counters: self.counters.snapshot(),
            memory: self.memory.snapshot(),
            cpu: self.cpu.snapshot(),
            stages: self.snapshots(),
            channels: self.channel_snapshots(),
        }
    }

    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        stage::Stage,
        telemetry::{component::StageComponent, run::RunTelemetry},
    };
    use std::{hint::black_box, sync::Arc, thread, time::Duration};

    #[test]
    fn concurrent_instances_keep_aggregate_and_instance_lifecycles() {
        let telemetry = RunTelemetry::new();
        let first = telemetry.start_stage_instance(Stage::Resolver, "resolver/input-1".to_string());
        let second = telemetry.start_stage_instance(Stage::Resolver, "resolver/lane-3".to_string());
        first.received(4);
        second.received(6);
        second.completed(6);
        drop(second);
        assert_eq!(telemetry.snapshots()[0].active_workers, 1);
        assert_eq!(telemetry.instance_snapshots().len(), 2);
        drop(first);
        assert_eq!(telemetry.snapshots()[0].active_workers, 0);
    }

    #[test]
    fn component_durations_are_recorded_and_surface_in_the_snapshot() {
        let telemetry = RunTelemetry::new();
        let stage = telemetry.start_stage(Stage::Resolver);
        stage.received(1);
        stage.completed(1);
        stage.component_time(StageComponent::StoreWrite, Duration::from_millis(30));
        stage.add_service_time(Duration::from_millis(40));
        drop(stage);

        let snapshot = telemetry.snapshots().into_iter().next().expect("one resolver stage");
        assert_eq!(snapshot.stage, Stage::Resolver);
        assert_eq!(snapshot.components.get(&StageComponent::StoreWrite), Some(&Duration::from_millis(30)));
        assert!(snapshot.service_time >= Duration::from_millis(40));
    }

    #[test]
    fn measure_service_records_cpu_time_for_a_busy_span() {
        let telemetry = RunTelemetry::new();
        let stage = telemetry.start_stage(Stage::Transformer);
        stage.measure_service(|| {
            let mut total: u64 = 0;
            for value in 0..20_000_000u64 {
                total = total.wrapping_add(black_box(value));
            }
            black_box(total);
        });
        drop(stage);
        let snapshot = telemetry.snapshots().into_iter().next().expect("one transformer stage");
        assert!(snapshot.service_cpu > Duration::ZERO, "service_cpu was {:?}", snapshot.service_cpu);
    }

    #[test]
    fn concurrent_received_calls_seed_the_first_input_instant_exactly_once() {
        let telemetry = Arc::new(RunTelemetry::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let telemetry = Arc::clone(&telemetry);
            handles.push(thread::spawn(move || {
                let stage = telemetry.start_stage(Stage::Extractor);
                for _ in 0..1_000 {
                    stage.received(1);
                    stage.completed(1);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("every worker finishes");
        }

        let snapshot = telemetry.snapshots().into_iter().next().expect("one extractor stage");
        assert_eq!(snapshot.received, 8_000);
        // A seeded first input yields a real average-throughput denominator; a lost or repeatedly
        // overwritten seed would collapse it to zero.
        assert!(snapshot.rates.average_throughput > 0.0);
    }

    #[test]
    fn stages_snapshot_in_pipeline_declaration_order() {
        let telemetry = RunTelemetry::new();
        drop(telemetry.start_stage(Stage::Writer));
        drop(telemetry.start_stage(Stage::Collector));
        let stages: Vec<Stage> = telemetry.snapshots().into_iter().map(|snapshot| snapshot.stage).collect();
        assert_eq!(stages, vec![Stage::Collector, Stage::Writer]);
    }
}

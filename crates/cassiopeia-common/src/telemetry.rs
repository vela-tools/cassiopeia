//! Run, stage, and channel throughput telemetry shared by the pipeline and writers.
//!
//! The subsystem is measurement-only: it records what each stage and channel did and produces an
//! immutable [`run::TelemetrySnapshot`], with no interpretation of which stage was the bottleneck.
//! Consumers import the concrete type from its owning module rather than through a re-export.

pub mod channel_boundary;
pub mod channel_metrics;
pub mod component;
pub mod cpu;
pub mod memory;
pub mod nanos;
pub mod rate;
pub mod run;
pub mod run_counters;
pub mod stage_metrics;

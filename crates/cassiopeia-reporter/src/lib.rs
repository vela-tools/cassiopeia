//! Reporter library for data processing pipelines.
//!
//! Provides a unified interface for reporting logs and progress across
//! different environments (interactive terminal, structured tracing logs).
//! The [`reporter::Reporter`] trait is the backend-agnostic contract;
//! [`backend`] holds the concrete terminal and tracing backends, and
//! [`builder::ReporterBuilder`] wires a backend into the global reporter.

pub mod animation;
pub mod backend;
pub mod builder;
pub mod error;
pub mod error_report;
pub mod global;
pub mod guard;
pub mod logging;
pub mod middleware;
pub mod reporter;
pub mod stage_handle;
pub mod stage_id;
pub mod trackers;

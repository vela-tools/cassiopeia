//! The per-stage worker threads the composition root wires together.
//!
//! Each module spawns one stage on its own thread and hands back the policy-controlled channel the next stage
//! reads from, so a run is a chain of threads connected by back-pressured [`Signal`](cassiopeia_common::signal::Signal)
//! channels.

pub(crate) mod aggregator;
pub(crate) mod assembler;
pub(crate) mod coded_error;
pub(crate) mod collector;
pub(crate) mod expander;
pub(crate) mod extractor;
pub(crate) mod ingestor;
pub(crate) mod nonconformant_types;
pub(crate) mod profiler;
pub(crate) mod pump;
pub(crate) mod resolver;
pub(crate) mod skipped_records;
pub(crate) mod stage_env;
pub(crate) mod stream_outcome;
pub(crate) mod transformer;
pub(crate) mod unreadable_timestamp_report;
pub(crate) mod validation_abort;
pub(crate) mod validation_decision;
pub(crate) mod validation_report;
pub(crate) mod validator;
pub(crate) mod writer;

//! The composition root that wires Cassiopeia's stages into a running pipeline.
//!
//! A [`Pipeline`](pipeline::Pipeline) is built from a manifest and an engine
//! [`PipelineConfig`](pipeline_config::PipelineConfig), then executed with
//! [`Pipeline::run`](pipeline::Pipeline::run). Each stage runs on its own thread, connected to the
//! next by a policy-controlled [`Signal`](cassiopeia_common::signal::Signal) channel, so the whole pipeline is
//! a back-pressured chain from collector to writer. A [`Schedule`](cassiopeia_manifest::schedule::Schedule)
//! decides how often the chain repeats, and a [`RunController`](controller::RunController) lets an
//! external signal cancel it mid-run.

mod completion;
mod context_resolution;
pub mod controller;
mod cycle;
pub mod error;
mod factories;
mod failure_code;
mod input_config;
mod memory_sampler;
pub mod observer;
mod phase1;
mod phase2;
pub mod pipeline;
pub mod pipeline_config;
pub mod pipeline_stage;
mod schedule_driver;
mod schema_resolution;
mod stages;

#[cfg(test)]
mod test_reporter;

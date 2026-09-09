//! Foundation types shared across the Cassiopeia workspace.
//!
//! This crate holds the small, dependency-light types every stage of the pipeline agrees on: the
//! [`signal::Signal`] envelope that carries data and control between stages, the source
//! [`format::DataFormat`] and [`input::Input`] descriptors, the NGSI-LD output knobs
//! ([`representation::NgsiLdRepresentation`], [`skip_null::NgsiLdSkipNull`],
//! [`context::mode::AtContextMode`]), the resolver [`store_kind::StoreKind`], and the foundation IO
//! and input error enums. It depends on nothing else in the workspace so every other crate can
//! depend on it.

pub mod attribute_overwrite;
pub mod batch;
pub mod broker_atomicity;
pub mod broker_header;
pub mod broker_operation;
pub mod captured_body;
pub mod channel;
pub mod collection;
pub mod context;
pub mod destination_kind;
pub mod error;
pub mod file_framing;
pub mod format;
pub mod input;
pub mod log;
pub mod memory_profile;
pub mod parallelism;
pub mod pipeline_mode;
pub mod representation;
pub mod run;
pub mod run_var;
pub mod schema_source;
pub mod signal;
pub mod skip_null;
pub mod stage;
pub mod store_kind;
pub mod telemetry;
pub mod tenant;
pub mod upsert_mode;
pub mod user_agent;

//! The closed code vocabulary: one enum per subsystem, summed by
//! [`diagnostic_code::DiagnosticCode`].
//!
//! A code is the stable, machine-readable name of *why* something failed. It is what repeats group
//! under in the run summary's reason table, so it is deliberately coarser than an error variant:
//! two errors a user would act on identically share one code.

pub mod broker_code;
pub mod catalog_code;
pub mod context_code;
pub mod diagnostic_code;
pub mod expander_code;
pub mod extractor_code;
pub mod geometry_code;
pub mod ingest_code;
pub mod run_code;
pub mod schema_code;
pub mod transform_code;

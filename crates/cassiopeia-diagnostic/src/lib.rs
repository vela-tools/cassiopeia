//! The diagnostic vocabulary: what failed, why, and what makes two failures the same failure.
//!
//! A stage builds a [`diagnostic::Diagnostic`] out of typed facts (a status, an endpoint, an entity
//! type, a JSON Pointer) rather than stringifying its error at the call site. That is what lets the
//! reporting layer afterwards tier the output by [`verbosity::Verbosity`], collapse repeats on
//! [`diagnostic_identity::DiagnosticIdentity`] instead of on rendered text, and fold a run's
//! failures into the summary's reason table via [`reason_tally::ReasonTally`].
//!
//! The crate is presentation-free: it owns no glyphs, no colours, and no layout. It sits below the
//! reporter and above the NGSI-LD model, so a diagnostic can name an entity type or an attribute
//! without either layer learning about the other.
//!
//! One concept per module: [`severity`] how serious, [`code`] the closed name vocabulary,
//! [`context_field`] the typed facts, [`field_role`] which of them define the failure,
//! [`json_pointer`]/[`detail`]/[`cause`]/[`schema_keyword`] the value types those facts carry,
//! [`diagnostic_identity`] the dedup and grouping key, [`diagnostic`] the reported failure,
//! [`diagnostic_builder`] how one is assembled, [`verbosity`] how much of it is rendered, and
//! [`reason`]/[`reason_tally`] the run-level fold.

pub mod cause;
pub mod code;
pub mod context_field;
pub mod detail;
pub mod diagnostic;
pub mod diagnostic_builder;
pub mod diagnostic_identity;
pub mod field_role;
pub mod json_pointer;
pub mod reason;
pub mod reason_tally;
pub mod schema_keyword;
pub mod severity;
pub mod verbosity;

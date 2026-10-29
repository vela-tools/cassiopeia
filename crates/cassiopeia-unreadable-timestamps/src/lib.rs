//! The record of source text a pipeline stage could not read as a timestamp.
//!
//! A timestamp Cassiopeia cannot read costs an entity something without failing it: an attribute
//! whose own value will not parse is never emitted, and an attribute whose `observedAt` or
//! `expiresAt` will not parse publishes with the qualifier missing (ETSI GS CIM 009 v1.9.1 clause
//! 4.5.2.2 with 4.22). Neither is worth failing a record over, and neither may pass silently, so
//! both are gathered here and named once the batch is through.
//!
//! The extract and transform stages each meet one of those losses, which is why the record lives in
//! a crate of its own rather than inside either of them. It knows nothing about diagnostics: it
//! gathers what was lost, and the stage that owns the loss decides what to call it.

pub mod timestamp_loss;
pub mod unreadable_timestamp;
pub mod unreadable_timestamps;

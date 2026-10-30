//! The record of source text a pipeline stage could not read as a timestamp.
//!
//! A timestamp Cassiopeia cannot read costs an entity something without failing it: an attribute
//! whose own value will not parse is never emitted, an attribute whose `observedAt` or `expiresAt`
//! will not parse publishes with the qualifier missing (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with
//! 4.22), and an entity whose `expiresAt` will not parse publishes without the member (clause
//! 4.5.1). None of these is worth failing a record over, and none may pass silently, so all are
//! gathered here and named once the batch is through.
//!
//! The extract, expand, and transform stages each meet one of those losses, which is why the record
//! lives in a crate of its own rather than inside any of them. Both sinks are a
//! [`BatchSink`](cassiopeia_batch_sink::batch_sink::BatchSink) keyed by what failed; they know
//! nothing about diagnostics: they gather what was lost, and the stage that owns the loss decides
//! what to call it.

pub mod attribute_loss;
pub mod timestamp_loss;
pub mod unreadable_expiries;
pub mod unreadable_expiry;
pub mod unreadable_timestamp;
pub mod unreadable_timestamps;

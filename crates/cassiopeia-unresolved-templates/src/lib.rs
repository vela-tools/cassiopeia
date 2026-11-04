//! The record of templates a pipeline stage could not render for a record.
//!
//! A template that fails to render for one record costs that record what the template declares, not
//! the whole entity: an attribute in the extract stage, a relationship link in the expand stage. Neither
//! loss is worth failing a record over, and neither may pass silently, so both are gathered here and
//! named once the batch is through.
//!
//! The extract and expand stages each meet the loss, which is why the record lives in a crate of its
//! own rather than inside either of them. The sink is a
//! [`BatchSink`](cassiopeia_batch_sink::batch_sink::BatchSink) keyed by what was lost; it knows nothing
//! about diagnostics: it gathers what was lost, and the stage that owns the loss decides what to call
//! it.

pub mod unresolved_template;
pub mod unresolved_templates;

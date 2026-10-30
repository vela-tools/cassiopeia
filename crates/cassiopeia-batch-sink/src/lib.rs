//! The failures one pipeline batch met, folded into one counted record per key.
//!
//! A stage that loses something to one record without failing it (an attribute dropped, a
//! qualifier left off, an entity member left out) must neither fail the record nor let the loss
//! pass silently. It records the loss here, under the key that names the failure, and the run names
//! each key once when the batch is through, with the count of records that shared it.
//!
//! Several stages meet such losses, which is why the sink lives in a crate of its own rather than
//! inside any of them. It knows nothing about diagnostics: it gathers what was lost, and the stage
//! that owns the loss decides what to call it.

pub mod batch_sink;
pub mod occurrences;
pub mod recurring;

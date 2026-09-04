//! Transformation stage for the Cassiopeia pipeline.
//!
//! The transformer takes a [`Mapped<Entity>`](cassiopeia_ir::mapped::Mapped) whose attribute values
//! and metadata have already been resolved by the extractor, and assembles an
//! [`NgsiLdEntity`](cassiopeia_ngsi_ld::entity::NgsiLdEntity): each mapping attribute becomes the
//! NGSI-LD attribute type its declaration calls for, carrying its resolved value and any
//! attribute-level metadata (`observedAt`, `unitCode`, `datasetId`, and custom properties).
//!
//! An `observedAt` whose text reads as no instant is dropped rather than emitted malformed, and
//! recorded in an
//! [`UnreadableTimestamps`](cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps)
//! sink the caller supplies: the attribute still publishes, and the run can name the qualifier it
//! lost instead of leaving the value with nothing anchoring it in time.

pub mod attribute_builder;
pub(crate) mod attribute_store;
pub mod error;
pub mod metadata;
pub mod ngsi_ld_transformer;
pub mod observed_at_cache;
pub mod qualifier_cache;
pub mod transformer;
pub mod unit_code_cache;

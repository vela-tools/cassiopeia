//! Transformation stage for the Cassiopeia pipeline.
//!
//! The transformer takes a [`Mapped<Entity>`](cassiopeia_ir::mapped::Mapped) whose attribute values
//! and metadata have already been resolved by the extractor, and assembles an
//! [`NgsiLdEntity`](cassiopeia_ngsi_ld::entity::NgsiLdEntity): each mapping attribute becomes the
//! NGSI-LD attribute type its declaration calls for, carrying its resolved value and any
//! attribute-level metadata (`observedAt`, `expiresAt`, `unitCode`, `datasetId`, and custom
//! properties).
//!
//! A qualifier Cassiopeia cannot write is left off rather than emitted malformed: an `observedAt` or
//! `expiresAt` whose text reads as no instant, and a `unitCode` that is no UN/CEFACT Recommendation
//! No. 20 code (ETSI GS CIM 009 v1.9.1 clause 4.5.2). Each is recorded in the
//! [`QualifierLosses`](crate::qualifier_losses::QualifierLosses) the caller supplies: the attribute
//! still publishes, and the run can name the qualifier it lost instead of leaving the value without
//! it unnoticed.

pub mod attribute_builder;
pub(crate) mod attribute_dispatch;
pub(crate) mod attribute_store;
pub mod error;
pub mod instant_cache;
pub mod metadata;
pub mod ngsi_ld_transformer;
pub(crate) mod property_instances;
pub mod qualifier_cache;
pub mod qualifier_losses;
pub(crate) mod relationship_assembly;
pub mod transformer;
pub mod unit_code_cache;
pub mod unknown_unit_code;
pub mod unknown_unit_codes;

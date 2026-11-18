//! Expansion stage of the Cassiopeia pipeline.
//!
//! The expander takes raw [`Record`](cassiopeia_ir::record::Record)s and, guided by a compiled
//! [`Mapping`](cassiopeia_mapping::mapping::Mapping), produces one or more
//! [`Fragment`](cassiopeia_ir::fragment::Fragment)s: the partial NGSI-LD entities the later stages
//! resolve, validate, and write. Identity, scope, and relationship URNs are minted here, and the
//! entity `expiresAt` is resolved.
//!
//! Each entity a record describes, the main one and every synthetic one, stands on its own: a failure
//! in an entity's own identity, scope, or `expiresAt` keeps that entity from being written, since
//! there is no entity without them, and costs no other entity of the record. A relationship links
//! only entities that are written. A relationship whose source, or whose synthetic entity's
//! identity, fails to render costs the entity that link and the synthetic entity it would have
//! named, not the entity itself: the loss is recorded in
//! [`ExpansionLosses`](crate::expansion_losses::ExpansionLosses), beside an entity `expiresAt` that
//! reads as no instant, and the entity is still expanded.

pub mod compiler;
pub mod error;
pub mod expander;
mod expansion;
pub mod expansion_losses;
pub mod expiry_resolver;
pub mod generic;
pub mod router;
pub mod urn;

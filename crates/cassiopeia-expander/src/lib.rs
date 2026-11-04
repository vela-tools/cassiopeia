//! Expansion stage of the Cassiopeia pipeline.
//!
//! The expander takes raw [`Record`](cassiopeia_ir::record::Record)s and, guided by a compiled
//! [`Mapping`](cassiopeia_mapping::mapping::Mapping), produces one or more
//! [`Fragment`](cassiopeia_ir::fragment::Fragment)s: the partial NGSI-LD entities the later stages
//! resolve, validate, and write. Identity, scope, and relationship URNs are minted here, and the
//! entity `expiresAt` is resolved.
//!
//! A relationship whose source, or whose synthetic entity's identity, fails to render costs the record
//! that link and the synthetic entity it would have named, not the record itself: the loss is recorded
//! in [`ExpansionLosses`](crate::expansion_losses::ExpansionLosses), beside an entity `expiresAt` that
//! reads as no instant, and the entity is still expanded. Only a failure in the entity's own identity
//! or scope fails the record, since there is no entity without them.

pub mod compiler;
pub mod error;
pub mod expander;
pub mod expansion_losses;
pub mod expiry_resolver;
pub mod generic;
pub mod router;
pub mod urn;

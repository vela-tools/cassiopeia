//! Expansion stage of the Cassiopeia pipeline.
//!
//! The expander takes raw [`Record`](cassiopeia_ir::record::Record)s and, guided by a compiled
//! [`Mapping`](cassiopeia_mapping::mapping::Mapping), produces one or more
//! [`Fragment`](cassiopeia_ir::fragment::Fragment)s: the partial NGSI-LD entities the later stages
//! resolve, validate, and write. Identity, scope, and relationship URNs are minted here.

pub mod compiler;
pub mod error;
pub mod expander;
pub mod generic;
pub mod router;
pub mod urn;

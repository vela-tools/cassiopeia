//! Intermediate representation carried between Cassiopeia's pipeline stages.
//!
//! A source payload becomes a [`record::Record`], is resolved into a
//! [`fragment::Fragment`], and finally an [`entity::Entity`] ready for NGSI-LD
//! serialization. [`mapped::Mapped`] pairs any of these with the mapping that
//! governs it, and [`payload`] models the collector/profiler hand-off.

pub mod assembled_entity;
pub mod entity;
pub mod error;
pub mod fragment;
pub mod mapped;
pub mod metadata;
pub mod parent_context;
pub mod payload;
pub mod payload_origin;
pub mod record;
pub mod relationship_path;
pub mod relationships;
pub mod sub_attribute;

//! Extraction stage for the Cassiopeia pipeline.
//!
//! The extractor takes a [`Mapped<Entity>`](cassiopeia_ir::mapped::Mapped) whose identity, scope,
//! and relationships have already been resolved, and fills in its NGSI-LD attribute values by
//! evaluating each attribute declaration in the mapping against the source record. Attribute-level
//! properties (such as `observedAt` or `unitCode`) are collected as metadata alongside the values.
//!
//! An attribute whose source value the declared transformation refuses is dropped rather than
//! emitted, and the refusal is recorded in a sink the caller supplies: a geometry the mapping did
//! not authorise converting in
//! [`DroppedGeometries`](crate::dropped_geometries::DroppedGeometries), text that reads as no
//! date-time in
//! [`UnreadableTimestamps`](cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps).
//! Either way the entity still reaches the writer and the run can report what it lost.

pub mod attribute;
pub mod dropped_geometries;
pub mod dropped_geometry;
pub mod entity_extractor;
pub mod error;
pub mod extractor;

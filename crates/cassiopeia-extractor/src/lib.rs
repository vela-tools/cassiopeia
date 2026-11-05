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
//! [`DroppedGeometries`](crate::dropped_geometries::DroppedGeometries), a geometry with a position
//! outside WGS 84 in [`OutOfRangeGeometries`](crate::out_of_range_geometries::OutOfRangeGeometries),
//! text that reads as no
//! date-time in
//! [`UnreadableTimestamps`](cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps),
//! and a `JsonProperty` source that is no JSON object in
//! [`RefusedJsonPayloads`](crate::refused_json_payloads::RefusedJsonPayloads). A `VocabProperty`
//! source that is no IRI, and a `LanguageProperty` source that is no map of text keyed by language
//! tag, are recorded in [`UnusableValues`](crate::unusable_values::UnusableValues), named by their
//! [`AttributePath`](crate::attribute_path::AttributePath). An attribute whose
//! template fails to render against the record is dropped the same way, and recorded in
//! [`UnresolvedTemplates`](cassiopeia_unresolved_templates::unresolved_templates::UnresolvedTemplates).
//!
//! No value holds a null: NGSI-LD admits none anywhere in an entity (ETSI GS CIM 009 v1.9.1 clause
//! 4.5.0), so a null nested in a source value is left out of it and recorded in
//! [`DroppedNulls`](crate::dropped_nulls::DroppedNulls), while the attribute is kept. Only a
//! `JsonProperty` keeps its source's nulls, because its `json` member is a JSON literal that NGSI-LD
//! never interprets (clause 4.5.24.2).
//!
//! All the sinks travel together as
//! [`DroppedAttributes`](crate::dropped_attributes::DroppedAttributes). Either way the entity still
//! reaches the writer and the run can report what it lost.

pub mod attribute;
pub mod attribute_path;
pub mod dropped_attributes;
pub mod dropped_geometries;
pub mod dropped_geometry;
pub mod dropped_null;
pub mod dropped_nulls;
pub mod entity_extractor;
pub mod error;
pub mod extractor;
pub mod out_of_range_geometries;
pub mod refused_json_payload;
pub mod refused_json_payloads;
pub mod required_value;
pub mod unusable_value;
pub mod unusable_values;

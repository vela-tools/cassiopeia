//! The NGSI-LD geometry domain.
//!
//! An NGSI-LD `GeoProperty` value is a `GeoJSON` geometry restricted to six types: `Point`,
//! `MultiPoint`, `LineString`, `MultiLineString`, `Polygon`, `MultiPolygon` (ETSI GS CIM 009
//! v1.9.1, clause 4.7). A `GeometryCollection`, which RFC 7946 clause 3.1.8 admits as a `GeoJSON`
//! geometry, is not one of them, so [`NgsiLdGeometry`](crate::geometry::NgsiLdGeometry) has no
//! variant for it and the RFC 7946 boundary refuses one.
//!
//! On top of that type the crate declares a conversion lattice: which source geometry may become
//! which target geometry, which of those conversions are lossless enough to apply automatically,
//! and which must be named in the mapping before any coordinate is discarded.

pub mod area_derivation;
pub mod convert;
pub mod coordinates;
pub mod error;
pub mod geometry;
pub mod lattice;
pub mod line_derivation;
pub mod measure;
pub mod normalisation;
pub mod planar;
pub mod point_derivation;
pub mod policy;
pub mod rfc7946;
pub mod ring;
pub mod selection;
pub mod strategy;
pub mod target;
pub mod validity;
pub mod vertex_derivation;

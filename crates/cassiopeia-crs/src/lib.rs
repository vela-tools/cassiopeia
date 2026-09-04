//! Coordinate reference system reprojection to EPSG:4326 (WGS84).
//!
//! NGSI-LD `GeoProperty` values are `GeoJSON` geometries per RFC 7946, whose coordinate reference
//! system is fixed to WGS84 with longitude/latitude order (RFC 7946 clause 4). Source geospatial
//! formats such as ESRI Shapefiles frequently carry coordinates in a projected or national CRS, so a
//! reprojection to EPSG:4326 is required before the geometry can be emitted as a spec-compliant
//! `GeoProperty`.
//!
//! This crate parses a source CRS from its WKT definition and reprojects arbitrary [`geo_types`]
//! geometries onto WGS84, using the pure-Rust `proj4rs`/`proj4wkt` stack so no native PROJ dependency is
//! linked.

pub mod error;
pub mod source;

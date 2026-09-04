use proj4rs::errors::Error as Proj4Error;

/// Errors that can occur while parsing a source CRS or reprojecting a geometry to EPSG:4326.
#[derive(Debug, thiserror::Error)]
pub enum CrsError {
    /// The `.prj` WKT could not be parsed into a proj4 definition.
    ///
    /// The message is the human-readable failure from `proj4wkt`; the crate exposes no public error
    /// type of its own to chain here.
    #[error("failed to parse the CRS WKT into a proj definition: {0}")]
    WktParse(String),

    /// A proj4 definition parsed from the WKT names a projection `proj4rs` cannot build (an unsupported
    /// CRS).
    #[error("unsupported CRS: failed to build a projection from '{proj_string}': {source}")]
    UnsupportedCrs { proj_string: String, source: Proj4Error },

    /// A coordinate could not be transformed to EPSG:4326.
    #[error("failed to transform a coordinate to EPSG:4326: {source}")]
    Transform { source: Proj4Error },
}

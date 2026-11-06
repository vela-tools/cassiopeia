use cassiopeia_crs::error::CrsError;

/// Errors that can occur while ingesting an OGC `GeoPackage`.
#[derive(Debug, thiserror::Error)]
pub enum GeoPackageIngestError {
    /// This build was compiled without the `geopackage` feature, so it links no SQLite and cannot read
    /// a `GeoPackage`.
    #[cfg(not(feature = "geopackage"))]
    #[error("this build of Cassiopeia was compiled without the `geopackage` feature, so it cannot read GeoPackage files")]
    NotBuiltIn,

    /// The `GeoPackage` ingestor was handed an in-memory byte payload; SQLite opens a database from a
    /// file.
    #[error("the GeoPackage ingestor requires a file, not bytes")]
    RequiresFile,

    /// `gpkg_contents` lists no `features` or `attributes` table, so the file holds no records.
    #[error("the GeoPackage holds no features or attributes table")]
    NoTables,

    /// A table's geometry column names an `srs_id` that `gpkg_spatial_ref_sys` does not define, so the
    /// coordinates cannot be placed in any coordinate reference system.
    #[error("the geometry column of the GeoPackage table '{table}' names srs_id {srs_id}, which gpkg_spatial_ref_sys does not define")]
    UnknownSrs { table: String, srs_id: i32 },

    /// A non-empty geometry has no `geo-types` (and so no `GeoJSON`) representation.
    #[error("feature {fid} of the GeoPackage table '{table}' holds a geometry that cannot be represented as GeoJSON")]
    UnrepresentableGeometry { table: String, fid: i64 },

    /// A column holds a value of a kind this version of Cassiopeia does not know how to read.
    #[error("column '{column}' of the GeoPackage table '{table}' holds a value of a kind Cassiopeia cannot read")]
    UnsupportedValue { table: String, column: String },

    /// The `geopackage` reader failed to open the file, read its catalogue, or read a row.
    ///
    /// The reader's error is boxed: inline it would make every `Result` carrying an
    /// [`crate::error::IngestorError`] several times larger than its other variants need.
    #[cfg(feature = "geopackage")]
    #[error(transparent)]
    Read(Box<geopackage::Error>),

    /// A table's CRS could not be parsed, or a geometry could not be reprojected to EPSG:4326.
    #[error(transparent)]
    Crs(#[from] CrsError),
}

// Written by hand because `thiserror`'s `#[from]` converts into the variant's field type as is and
// cannot box it; this keeps `?` working on the reader's results.
#[cfg(feature = "geopackage")]
impl From<geopackage::Error> for GeoPackageIngestError {
    fn from(error: geopackage::Error) -> GeoPackageIngestError {
        GeoPackageIngestError::Read(Box::new(error))
    }
}

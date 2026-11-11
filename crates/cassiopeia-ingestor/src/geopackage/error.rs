use crate::geopackage::unreadable_table::UnreadableTable;
use cassiopeia_crs::error::CrsError;
#[cfg(feature = "geopackage")]
use std::{io, path::PathBuf};

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

    /// A write-ahead log sits beside the `GeoPackage`, so the file is not opened: the bundled SQLite,
    /// and a system SQLite older than 3.54.0, predate the fix for RUSTSEC-2026-0459 (CVE-2026-119375),
    /// under which a log whose page size differs from the database's makes queries return
    /// uninitialised memory as row data.
    #[cfg(feature = "geopackage")]
    #[error(
        "the GeoPackage has a write-ahead log beside it at '{}', and Cassiopeia does not read a GeoPackage together with its log (RUSTSEC-2026-0459); close the program that holds the GeoPackage open so the log is merged back into it, then run again",
        .log.display()
    )]
    WriteAheadLog { log: PathBuf },

    /// Whether a write-ahead log sits beside the `GeoPackage` could not be determined, so the file is
    /// not opened.
    #[cfg(feature = "geopackage")]
    #[error("cannot determine whether a write-ahead log sits beside the GeoPackage '{}'", .database.display())]
    WriteAheadLogUnverified {
        database: PathBuf,
        #[source]
        source: io::Error,
    },

    /// `gpkg_contents` lists no `features` or `attributes` table, so the file holds no records.
    #[error("the GeoPackage holds no features or attributes table")]
    NoTables,

    /// Every record table the run reads failed before its first row was read, each skipped with a
    /// warning, so the file yielded no records. The first table to fail is the source.
    #[error("no record table of the GeoPackage can be read")]
    NoReadableTable(#[source] UnreadableTable),

    /// `gpkg_contents` lists a table as `features`, but `gpkg_geometry_columns` has no row for its
    /// geometry column, so its geometries cannot be told from its other columns.
    #[error("the GeoPackage table '{table}' is listed as features, but gpkg_geometry_columns names no geometry column for it")]
    NoGeometryColumn { table: String },

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

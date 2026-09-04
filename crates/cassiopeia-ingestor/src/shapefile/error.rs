use cassiopeia_crs::error::CrsError;
use std::{io, path::PathBuf};
use zip::result::ZipError;

/// Errors that can occur while ingesting an ESRI Shapefile, in either carrier (bare `.shp` or zip
/// bundle).
#[derive(Debug, thiserror::Error)]
pub enum ShapefileIngestError {
    /// The shapefile ingestor was handed an in-memory byte payload; it needs a file so the reader can
    /// resolve the companion `.dbf`/`.shx`/`.prj`/`.cpg` siblings from disk.
    #[error("the shapefile ingestor requires a file, not bytes")]
    RequiresFile,

    /// The carrier's leading bytes matched neither the `.shp` file code nor the zip local-file magic.
    #[error("the shapefile carrier is neither a bare .shp nor a zip bundle")]
    UnknownCarrier,

    /// A `.shp` main file has no companion `.dbf`: attribute records cannot be read.
    #[error("the shapefile layer '{layer}' has no companion .dbf attribute file")]
    MissingDbf { layer: String },

    /// The carrier held no `.shp` layer at all.
    #[error("no shapefile layer was found in the input")]
    NoLayers,

    /// An I/O error occurred while reading a companion file or extracting the zip bundle.
    #[error("I/O error at '{}'", path.display())]
    Io { source: io::Error, path: PathBuf },

    /// The `shapefile`/`dbase` reader failed to open or parse a layer.
    #[error(transparent)]
    Read(#[from] shapefile::Error),

    /// The zip bundle could not be opened or a member could not be extracted.
    #[error(transparent)]
    Zip(#[from] ZipError),

    /// A geometry could not be reprojected to EPSG:4326.
    #[error(transparent)]
    Crs(#[from] CrsError),
}

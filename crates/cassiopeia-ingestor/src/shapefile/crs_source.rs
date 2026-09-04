use crate::shapefile::error::ShapefileIngestError;
use cassiopeia_crs::source::SourceCrs;
use std::{fs, io::ErrorKind, path::Path};

/// Reads the `.prj` sibling of a `.shp` layer and builds its source CRS for reprojection.
///
/// An absent `.prj` yields `Ok(None)`: by common real-world convention the coordinates are then assumed
/// to already be EPSG:4326 and are emitted unchanged, and a warning is logged. A present `.prj` is
/// parsed from its WKT; a non-4326 CRS is reprojected downstream, an already-4326 one round-trips as an
/// identity.
///
/// # Errors
///
/// Returns [`ShapefileIngestError::Io`] when the `.prj` exists but cannot be read, or
/// [`ShapefileIngestError::Crs`] when its WKT cannot be parsed into a supported CRS.
pub fn read_source_crs(shp_path: &Path) -> Result<Option<SourceCrs>, ShapefileIngestError> {
    let prj_path = shp_path.with_extension("prj");
    let wkt = match fs::read_to_string(&prj_path) {
        Ok(wkt) => wkt,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            tracing::warn!(
                shapefile = %shp_path.display(),
                "shapefile has no .prj sibling; assuming EPSG:4326 and emitting coordinates unchanged"
            );
            return Ok(None);
        }
        Err(source) => return Err(ShapefileIngestError::Io { source, path: prj_path }),
    };

    let crs = SourceCrs::from_wkt(&wkt)?;
    Ok(Some(crs))
}

use crate::geopackage::error::GeoPackageIngestError;
use cassiopeia_crs::source::SourceCrs;
use geopackage::{GeoPackage, Layer, Srs};

/// The definition `gpkg_spatial_ref_sys` stores for a CRS it cannot describe: the required `srs_id`
/// rows -1 (undefined Cartesian) and 0 (undefined geographic) carry it (OGC 12-128r19 Requirement 11).
const UNDEFINED_DEFINITION: &str = "undefined";

/// Reads the CRS of a table's geometry column from `gpkg_spatial_ref_sys` and builds its source CRS
/// for reprojection.
///
/// Returns `Ok(None)` when no reprojection is needed or possible:
///
/// - an `attributes` table, which has no geometry;
/// - an EPSG:4326 column, whose coordinates are already the WGS84 longitude and latitude RFC 7946
///   clause 4 requires (a `GeoPackage` geometry stores x before y, so longitude comes first);
/// - an `undefined` CRS, whose coordinates are assumed to be EPSG:4326 and emitted unchanged, with a
///   warning, matching how a shapefile without a `.prj` is read.
///
/// Otherwise the WKT2 definition of the `gpkg_crs_wkt` extension is preferred over the WKT1 one when
/// both are present, as that extension directs (OGC 12-128r19 Annex F.10).
///
/// # Errors
///
/// Returns [`GeoPackageIngestError::UnknownSrs`] when the column's `srs_id` has no
/// `gpkg_spatial_ref_sys` row, [`GeoPackageIngestError::Read`] when that table cannot be read, or
/// [`GeoPackageIngestError::Crs`] when the definition cannot be parsed into a supported CRS.
pub fn read_source_crs(gpkg: &GeoPackage, layer: &Layer<'_>) -> Result<Option<SourceCrs>, GeoPackageIngestError> {
    let Some(geometry_column) = layer.geometry_column() else {
        return Ok(None);
    };
    let srs_id = geometry_column.srs_id;
    let srs = gpkg.srs(srs_id)?.ok_or_else(|| GeoPackageIngestError::UnknownSrs {
        table: layer.table_name().to_string(),
        srs_id,
    })?;
    if is_wgs84(&srs) {
        return Ok(None);
    }

    let definition = srs.definition_wkt2.as_deref().unwrap_or(&srs.definition);
    if definition.trim().eq_ignore_ascii_case(UNDEFINED_DEFINITION) {
        tracing::warn!(
            table = layer.table_name(),
            srs_id,
            "GeoPackage table has an undefined coordinate reference system; assuming EPSG:4326 and emitting coordinates unchanged"
        );
        return Ok(None);
    }

    Ok(Some(SourceCrs::from_wkt(definition)?))
}

/// Whether `srs` is EPSG:4326, the WGS84 geographic CRS every emitted geometry is expressed in.
fn is_wgs84(srs: &Srs) -> bool {
    srs.organization.eq_ignore_ascii_case("EPSG") && srs.organization_coordsys_id == 4326
}

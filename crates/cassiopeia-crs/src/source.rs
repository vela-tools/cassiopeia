use crate::error::CrsError;
use geo::MapCoords;
use geo_types::{Coord, Geometry};
use proj4rs::{Proj, transform::transform};

/// The proj4 definition of EPSG:4326 (WGS84 geographic, longitude/latitude in degrees).
const WGS84_PROJ_STRING: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// A source coordinate reference system, ready to reproject geometries onto EPSG:4326 (WGS84).
///
/// Built from a `.prj` WKT definition or a proj4 string, it holds the source projection alongside a
/// prepared WGS84 target so every reprojected geometry reuses the same two projections.
#[derive(Debug)]
pub struct SourceCrs {
    source: Proj,
    target: Proj,
    source_is_geographic: bool,
}

impl SourceCrs {
    /// Builds a source CRS from a `.prj` WKT definition (WKT1 or WKT2).
    ///
    /// # Errors
    ///
    /// Returns [`CrsError::WktParse`] when the WKT cannot be translated to a proj4 definition, or
    /// [`CrsError::UnsupportedCrs`] when the resulting definition names a projection `proj4rs` cannot
    /// build.
    pub fn from_wkt(wkt: &str) -> Result<SourceCrs, CrsError> {
        let proj_string = proj4wkt::wkt_to_projstring(wkt).map_err(|error| CrsError::WktParse(error.to_string()))?;
        SourceCrs::from_proj_string(&proj_string)
    }

    /// Builds a source CRS directly from a proj4 definition string.
    ///
    /// # Errors
    ///
    /// Returns [`CrsError::UnsupportedCrs`] when either the source definition or the built-in WGS84
    /// target cannot be built by `proj4rs`.
    pub fn from_proj_string(proj_string: &str) -> Result<SourceCrs, CrsError> {
        let source = Proj::from_proj_string(proj_string).map_err(|source| CrsError::UnsupportedCrs {
            proj_string: proj_string.to_string(),
            source,
        })?;
        let target = Proj::from_proj_string(WGS84_PROJ_STRING).map_err(|source| CrsError::UnsupportedCrs {
            proj_string: WGS84_PROJ_STRING.to_string(),
            source,
        })?;
        let source_is_geographic = source.is_latlong();
        Ok(SourceCrs {
            source,
            target,
            source_is_geographic,
        })
    }

    /// Reprojects every coordinate of `geometry` from the source CRS onto EPSG:4326 (WGS84), returning
    /// the geometry with longitude/latitude coordinates in degrees.
    ///
    /// # Errors
    ///
    /// Returns [`CrsError::Transform`] when a coordinate cannot be transformed.
    pub fn reproject(&self, geometry: &Geometry<f64>) -> Result<Geometry<f64>, CrsError> {
        geometry.try_map_coords(|coord| self.reproject_coord(coord))
    }

    /// Reprojects a single coordinate onto WGS84.
    ///
    /// `proj4rs` works in radians for geographic CRSs (ETSI/RFC coordinates are degrees): a geographic
    /// source's degrees are converted to radians before transform, and the WGS84 target (always
    /// geographic) returns radians that are converted back to degrees.
    fn reproject_coord(&self, coord: Coord<f64>) -> Result<Coord<f64>, CrsError> {
        let mut point = if self.source_is_geographic {
            (coord.x.to_radians(), coord.y.to_radians())
        } else {
            (coord.x, coord.y)
        };
        transform(&self.source, &self.target, &mut point).map_err(|source| CrsError::Transform { source })?;
        Ok(Coord {
            x: point.0.to_degrees(),
            y: point.1.to_degrees(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::CrsError, source::SourceCrs};
    use geo_types::{Geometry, Point};

    /// EPSG:3857 (Web Mercator) on a WGS84 sphere: `x = lon * a * pi / 180`, so an equatorial point at
    /// x = 1113194.9079 m reprojects to lon 10 deg, lat 0.
    const WEB_MERCATOR: &str = "+proj=merc +a=6378137 +b=6378137 +lon_0=0 +x_0=0 +y_0=0 +units=m +no_defs";

    /// EPSG:27700 (British National Grid) with the OSGB36 seven-parameter Helmert shift to WGS84.
    const BRITISH_NATIONAL_GRID: &str = "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 +ellps=airy +towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489 +units=m +no_defs";

    /// A plain GEOGCS WGS84 WKT, so the WKT parsing path is exercised for an already-4326 source.
    const WGS84_WKT: &str =
        r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

    fn point(crs: &SourceCrs, x: f64, y: f64) -> (f64, f64) {
        let reprojected = crs.reproject(&Geometry::Point(Point::new(x, y))).unwrap();
        let Geometry::Point(p) = reprojected else {
            panic!("a reprojected point stays a point");
        };
        (p.x(), p.y())
    }

    #[test]
    fn a_web_mercator_point_reprojects_to_the_expected_wgs84_lon_lat() {
        let crs = SourceCrs::from_proj_string(WEB_MERCATOR).unwrap();
        let (lon, lat) = point(&crs, 1_113_194.907_9, 0.0);
        assert!((lon - 10.0).abs() < 1e-6, "lon was {lon}");
        assert!(lat.abs() < 1e-6, "lat was {lat}");
    }

    #[test]
    fn a_british_national_grid_control_point_reprojects_within_tolerance() {
        // Ordnance Survey worked example: E 651409.903, N 313177.270 -> lat 52.6575703, lon 1.7179215.
        // That reference is OSTN-grid accurate (cm); the pure seven-parameter Helmert shift used here
        // diverges by roughly 100 m, so the tolerance is loosened accordingly. The point of the test is
        // that a projected national grid lands near the right place, not sub-metre fidelity.
        let crs = SourceCrs::from_proj_string(BRITISH_NATIONAL_GRID).unwrap();
        let (lon, lat) = point(&crs, 651_409.903, 313_177.270);
        assert!((lon - 1.717_921_5).abs() < 5e-3, "lon was {lon}");
        assert!((lat - 52.657_570_3).abs() < 5e-3, "lat was {lat}");
    }

    #[test]
    fn an_already_wgs84_wkt_reprojects_as_an_identity() {
        let crs = SourceCrs::from_wkt(WGS84_WKT).unwrap();
        let (lon, lat) = point(&crs, 14.5, 46.05);
        assert!((lon - 14.5).abs() < 1e-9, "lon was {lon}");
        assert!((lat - 46.05).abs() < 1e-9, "lat was {lat}");
    }

    #[test]
    fn an_invalid_wkt_is_a_typed_error_and_does_not_panic() {
        let error = SourceCrs::from_wkt("this is not wkt").unwrap_err();
        assert!(matches!(error, CrsError::WktParse(_) | CrsError::UnsupportedCrs { .. }));
    }
}

use crate::error::CrsError;
use geo::MapCoords;
use geo_types::{Coord, Geometry};
use proj4rs::{Proj, transform::transform};

/// The proj4 definition of EPSG:4326 (WGS84 geographic, longitude/latitude in degrees).
const WGS84_PROJ_STRING: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// The unit a source CRS stores its coordinates in, which decides how they are handed to `proj4rs`.
#[derive(Debug, Clone, Copy)]
enum SourceUnit {
    /// Degrees of longitude and latitude, as a geographic CRS stores them, which `proj4rs` reads in
    /// radians.
    Degrees,
    /// The linear unit of a projected or geocentric CRS, which `proj4rs` reads as stored.
    Linear,
}

/// A source coordinate reference system, ready to reproject geometries onto EPSG:4326 (WGS84).
///
/// Built from a `.prj` WKT definition or a proj4 string, it holds the source projection alongside a
/// prepared WGS84 target so every reprojected geometry reuses the same two projections.
#[derive(Debug)]
pub struct SourceCrs {
    source: Proj,
    target: Proj,
    source_unit: SourceUnit,
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
        let source_unit = if source.is_latlong() { SourceUnit::Degrees } else { SourceUnit::Linear };
        Ok(SourceCrs { source, target, source_unit })
    }

    /// Reprojects every coordinate of `geometry` from the source CRS onto EPSG:4326 (WGS84), returning
    /// the geometry with longitude/latitude coordinates in degrees.
    ///
    /// A coordinate the transformation leaves where it is keeps the degrees the source stored, so a
    /// geographic CRS that differs from WGS84 by a null transformation, such as ETRS89 (EPSG
    /// transformation 1149), has its coordinates returned exactly as they were.
    ///
    /// # Errors
    ///
    /// Returns [`CrsError::Transform`] when a coordinate cannot be transformed.
    pub fn reproject(&self, geometry: &Geometry<f64>) -> Result<Geometry<f64>, CrsError> {
        geometry.try_map_coords(|coord| self.reproject_coord(coord))
    }

    /// Reprojects a single coordinate onto WGS84.
    ///
    /// `proj4rs` reads and writes a geographic CRS's coordinates in radians, while a geographic source
    /// and RFC 7946 store degrees: a geographic source's degrees are converted to radians before the
    /// transformation, and the WGS84 radians it returns are converted back to degrees.
    fn reproject_coord(&self, coord: Coord<f64>) -> Result<Coord<f64>, CrsError> {
        match self.source_unit {
            SourceUnit::Degrees => {
                let radians = (coord.x.to_radians(), coord.y.to_radians());
                let reprojected = self.to_wgs84(radians)?;
                // Degrees do not survive the trip to radians and back: 14.5058 returns as
                // 14.505799999999999. A coordinate the transformation returns bit for bit is one it does
                // not move, so it keeps the degrees the source stored.
                if bit_patterns(reprojected) == bit_patterns(radians) {
                    Ok(coord)
                } else {
                    Ok(degrees(reprojected))
                }
            }
            SourceUnit::Linear => self.to_wgs84((coord.x, coord.y)).map(degrees),
        }
    }

    /// Transforms `point`, in the units `proj4rs` reads the source CRS in, onto WGS84 longitude and
    /// latitude in radians.
    fn to_wgs84(&self, mut point: (f64, f64)) -> Result<(f64, f64), CrsError> {
        transform(&self.source, &self.target, &mut point).map_err(|source| CrsError::Transform { source })?;
        Ok(point)
    }
}

/// The coordinate in degrees of a WGS84 longitude and latitude in radians.
const fn degrees((longitude, latitude): (f64, f64)) -> Coord<f64> {
    Coord {
        x: longitude.to_degrees(),
        y: latitude.to_degrees(),
    }
}

/// The bit patterns of a point's two values, which are equal only for the same values bit for bit.
const fn bit_patterns((x, y): (f64, f64)) -> (u64, u64) {
    (x.to_bits(), y.to_bits())
}

#[cfg(test)]
mod tests {
    use crate::{error::CrsError, source::SourceCrs};
    use geo_types::{Geometry, Point, line_string};

    /// EPSG:3857 (Web Mercator) on a WGS84 sphere: `x = lon * a * pi / 180`, so an equatorial point at
    /// x = 1113194.9079 m reprojects to lon 10 deg, lat 0.
    const WEB_MERCATOR: &str = "+proj=merc +a=6378137 +b=6378137 +lon_0=0 +x_0=0 +y_0=0 +units=m +no_defs";

    /// EPSG:27700 (British National Grid) with the OSGB36 seven-parameter Helmert shift to WGS84.
    const BRITISH_NATIONAL_GRID: &str = "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 +ellps=airy +towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489 +units=m +no_defs";

    /// A plain GEOGCS WGS84 WKT, so the WKT parsing path is exercised for an already-4326 source.
    const WGS84_WKT: &str =
        r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

    /// EPSG:4258 (ETRS89) as GDAL writes it into a `GeoPackage`: no `TOWGS84`, latitude axis first.
    const ETRS89_WKT: &str = r#"GEOGCS["ETRS89",DATUM["European_Terrestrial_Reference_System_1989",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6258"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Latitude",NORTH],AXIS["Longitude",EAST],AUTHORITY["EPSG","4258"]]"#;

    /// EPSG:4258 (ETRS89) with the zero shift of EPSG transformation 1149 spelled out as `TOWGS84`.
    const ETRS89_ZERO_TOWGS84_WKT: &str = r#"GEOGCS["ETRS89",DATUM["European_Terrestrial_Reference_System_1989",SPHEROID["GRS 1980",6378137,298.257222101],TOWGS84[0,0,0,0,0,0,0]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

    /// EPSG:4230 (ED50) with a three-parameter shift to WGS84 of (-87, -98, -121) m.
    const ED50_WKT: &str = r#"GEOGCS["ED50",DATUM["European_Datum_1950",SPHEROID["International 1924",6378388,297],TOWGS84[-87,-98,-121,0,0,0,0]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

    /// Ljubljana, whose longitude does not survive the trip from degrees to radians and back.
    const LJUBLJANA: (f64, f64) = (14.5058, 46.0569);

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
    fn the_trip_from_degrees_to_radians_and_back_changes_the_fixture_longitude() {
        assert_eq!(LJUBLJANA.0.to_radians().to_degrees(), 14.505_799_999_999_999);
    }

    #[test]
    fn an_already_wgs84_point_keeps_the_degrees_the_source_stored() {
        let crs = SourceCrs::from_wkt(WGS84_WKT).unwrap();

        assert_eq!(point(&crs, LJUBLJANA.0, LJUBLJANA.1), LJUBLJANA);
    }

    #[test]
    fn an_etrs89_point_as_gdal_writes_its_crs_keeps_the_degrees_the_source_stored() {
        let crs = SourceCrs::from_wkt(ETRS89_WKT).unwrap();

        assert_eq!(point(&crs, LJUBLJANA.0, LJUBLJANA.1), LJUBLJANA);
    }

    #[test]
    fn an_etrs89_point_with_a_zero_towgs84_keeps_the_degrees_the_source_stored() {
        let crs = SourceCrs::from_wkt(ETRS89_ZERO_TOWGS84_WKT).unwrap();

        assert_eq!(point(&crs, LJUBLJANA.0, LJUBLJANA.1), LJUBLJANA);
    }

    #[test]
    fn every_coordinate_of_a_geometry_a_null_transformation_leaves_in_place_keeps_its_source_degrees() {
        let crs = SourceCrs::from_wkt(ETRS89_WKT).unwrap();
        let geometry = Geometry::LineString(line_string![
            (x: 14.5058, y: 46.0569),
            (x: -0.1278, y: 51.5074),
            (x: 180.0, y: -90.0),
            (x: -179.999_999_9, y: 89.999_999_9),
        ]);

        assert_eq!(crs.reproject(&geometry).unwrap(), geometry);
    }

    #[test]
    fn a_geographic_point_a_datum_shift_moves_is_written_in_the_shifted_degrees() {
        // PROJ's cs2cs, given the same three-parameter shift: 14.504855581109, 46.056027650618.
        let crs = SourceCrs::from_wkt(ED50_WKT).unwrap();
        let (lon, lat) = point(&crs, LJUBLJANA.0, LJUBLJANA.1);
        assert!((lon - 14.504_855_581_109).abs() < 1e-9, "lon was {lon}");
        assert!((lat - 46.056_027_650_618).abs() < 1e-9, "lat was {lat}");
    }

    #[test]
    fn an_invalid_wkt_is_a_typed_error_and_does_not_panic() {
        let error = SourceCrs::from_wkt("this is not wkt").unwrap_err();
        assert!(matches!(error, CrsError::WktParse(_) | CrsError::UnsupportedCrs { .. }));
    }
}

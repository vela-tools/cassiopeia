use cassiopeia_crs::{error::CrsError, source::SourceCrs};
use geo_types::Geometry;
use geojson::{Geometry as GeoJsonGeometry, GeometryValue};
use serde_json::Value;

/// Converts a source geometry into the `GeoJSON` value a record carries under `geometry`, reprojected
/// onto EPSG:4326 when its source CRS is known.
///
/// NGSI-LD `GeoProperty` values are `GeoJSON` geometries, whose coordinates are WGS84 longitude and
/// latitude (RFC 7946 clause 4), so a geometry read in a projected or national CRS is reprojected
/// before it is emitted. `crs` is `None` when the coordinates are already, or are assumed to be,
/// EPSG:4326; they are then emitted unchanged.
///
/// Returns `Ok(None)` when the geometry cannot be serialised as `GeoJSON`.
///
/// # Errors
///
/// Returns [`CrsError`] when a coordinate cannot be reprojected to EPSG:4326.
pub fn wgs84_geometry_value(geometry: Geometry<f64>, crs: Option<&SourceCrs>) -> Result<Option<Value>, CrsError> {
    let geometry = match crs {
        Some(crs) => crs.reproject(&geometry)?,
        None => geometry,
    };

    let geojson_geometry = GeoJsonGeometry::new(GeometryValue::from(&geometry));
    Ok(serde_json::to_value(&geojson_geometry).ok())
}

#[cfg(test)]
mod tests {
    use crate::wgs84_geometry::wgs84_geometry_value;
    use cassiopeia_crs::source::SourceCrs;
    use geo_types::{Geometry, Point};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// EPSG:3857 on a WGS84 sphere: an equatorial easting of 1113194.9079 m is longitude 10.
    const WEB_MERCATOR: &str = "+proj=merc +a=6378137 +b=6378137 +lon_0=0 +x_0=0 +y_0=0 +units=m +no_defs";

    fn coordinates(value: &Value) -> (f64, f64) {
        let coordinates = value.get("coordinates").and_then(Value::as_array).unwrap();
        (coordinates[0].as_f64().unwrap(), coordinates[1].as_f64().unwrap())
    }

    #[test]
    fn a_geometry_without_a_source_crs_is_emitted_unchanged() {
        let value = wgs84_geometry_value(Geometry::Point(Point::new(14.5, 46.05)), None).unwrap().unwrap();
        assert_eq!(value, json!({ "type": "Point", "coordinates": [14.5, 46.05] }));
    }

    #[test]
    fn a_geometry_with_a_projected_source_crs_is_reprojected_to_wgs84() {
        let crs = SourceCrs::from_proj_string(WEB_MERCATOR).unwrap();
        let value = wgs84_geometry_value(Geometry::Point(Point::new(1_113_194.907_9, 0.0)), Some(&crs))
            .unwrap()
            .unwrap();

        assert_eq!(value.get("type").and_then(Value::as_str), Some("Point"));
        let (lon, lat) = coordinates(&value);
        assert!((lon - 10.0).abs() < 1e-6, "lon was {lon}");
        assert!(lat.abs() < 1e-6, "lat was {lat}");
    }
}

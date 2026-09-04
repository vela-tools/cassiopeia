pub mod atan2;
pub mod bearing;
pub mod clamp;
pub mod constant;
pub mod dms_point;
pub mod geo_area;
pub mod geo_bbox;
pub mod geo_centroid;
pub mod geo_convert;
pub mod geo_length;
pub mod geohash;
pub mod geometry_argument;
pub mod hypot;
pub mod map_range;
pub mod wind;

use crate::template::function::{
    atan2::atan2,
    bearing::bearing,
    clamp::clamp,
    constant::{e, pi, tau},
    dms_point::dms_point,
    geo_area::geo_area,
    geo_bbox::geo_bbox,
    geo_centroid::geo_centroid,
    geo_convert::geo_convert,
    geo_length::geo_length,
    geohash::geohash,
    hypot::hypot,
    map_range::map_range,
    wind::{wind_direction, wind_speed},
};
use tera::Tera;

/// Registers every Cassiopeia-defined Tera function on an engine.
///
/// Kept in one place, alongside the filter registrar, so a newly added function is available to
/// every engine the crate builds rather than only to whichever construction path happened to be
/// updated.
pub fn register(tera: &mut Tera) {
    tera.register_function("dms_point", dms_point);
    tera.register_function("geohash", geohash);
    tera.register_function("geo_convert", geo_convert);
    tera.register_function("geo_centroid", geo_centroid);
    tera.register_function("geo_bbox", geo_bbox);
    tera.register_function("geo_area", geo_area);
    tera.register_function("geo_length", geo_length);
    tera.register_function("hypot", hypot);
    tera.register_function("clamp", clamp);
    tera.register_function("map_range", map_range);
    tera.register_function("atan2", atan2);
    tera.register_function("pi", pi);
    tera.register_function("tau", tau);
    tera.register_function("e", e);
    tera.register_function("bearing", bearing);
    tera.register_function("wind_speed", wind_speed);
    tera.register_function("wind_direction", wind_direction);
}

#[cfg(test)]
mod tests {
    use crate::template::function::register;
    use std::f64::consts::PI;
    use tera::{Context, Tera};

    #[test]
    fn the_geohash_function_is_registered() {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", "{{ geohash(lat=35.3003, lon=-120.6623, precision=5) }}").unwrap();

        assert_eq!(tera.render("t", &Context::new()).unwrap(), "9q60y");
    }

    #[test]
    fn the_dms_point_function_is_registered() {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", r#"{{ dms_point(value="27°59′17″N 86°55′30″E") }}"#).unwrap();

        let rendered = tera.render("t", &Context::new()).unwrap();
        let point: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(point.get("type").unwrap(), "Point");
        let coordinates = point.get("coordinates").unwrap().as_array().unwrap();
        assert!((coordinates[0].as_f64().unwrap() - 86.925).abs() < 1e-6);
        assert!((coordinates[1].as_f64().unwrap() - 27.988_055_6).abs() < 1e-6);
    }

    /// A `GeoJSON` `MultiPolygon` of two square surfaces, the second three times the side of the first.
    fn multi_polygon() -> tera::Value {
        let document = serde_json::json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[10.0, 10.0], [13.0, 10.0], [13.0, 13.0], [10.0, 10.0]]],
            ],
        });

        tera::Value::try_from_serializable(&document).unwrap()
    }

    /// Renders `template` with the two-surface `MultiPolygon` bound as `geometry`.
    fn with_geometry(template: &str) -> String {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", multi_polygon());

        tera.render("t", &context).unwrap()
    }

    #[test]
    fn the_geo_convert_function_is_registered() {
        assert_eq!(
            with_geometry(r#"{% set converted = geo_convert(value=geometry, to="polygon", using="largest") %}{{ converted.type }}"#),
            "Polygon"
        );
    }

    #[test]
    fn the_geo_centroid_function_is_registered() {
        assert_eq!(with_geometry("{% set point = geo_centroid(value=geometry) %}{{ point.type }}"), "Point");
    }

    #[test]
    fn the_geo_bbox_function_is_registered() {
        assert_eq!(with_geometry("{% set box = geo_bbox(value=geometry) %}{{ box.type }}"), "Polygon");
    }

    #[test]
    fn the_geo_area_function_is_registered() {
        assert!(with_geometry("{{ geo_area(value=geometry) }}").parse::<f64>().unwrap() > 0.0);
    }

    #[test]
    fn the_geo_length_function_is_registered() {
        assert!(with_geometry("{{ geo_length(value=geometry) }}").parse::<f64>().unwrap() > 0.0);
    }

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn the_hypot_function_is_registered() {
        assert!((number("{{ hypot(x=3, y=4) }}") - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_clamp_function_is_registered() {
        assert!((number("{{ clamp(value=5, min=0, max=1) }}") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_map_range_function_is_registered() {
        assert!((number("{{ map_range(value=5, in_min=0, in_max=10, out_min=0, out_max=100) }}") - 50.0).abs() < 1e-9);
    }

    #[test]
    fn the_atan2_function_is_registered() {
        assert!(number("{{ atan2(y=0, x=1) }}").abs() < 1e-9);
    }

    #[test]
    fn the_constant_functions_are_registered() {
        assert!((number("{{ pi() }}") - PI).abs() < 1e-12);
    }

    #[test]
    fn the_bearing_function_is_registered() {
        assert!((number("{{ bearing(east=1, north=0) }}") - 270.0).abs() < 1e-9);
    }

    #[test]
    fn the_wind_functions_are_registered() {
        assert!((number("{{ wind_speed(u=3, v=4) }}") - 5.0).abs() < 1e-9);
        assert!((number("{{ wind_direction(u=0, v=1) }}") - 180.0).abs() < 1e-9);
    }
}

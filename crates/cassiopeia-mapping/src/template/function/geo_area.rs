use crate::template::{
    function::geometry_argument::{GeometryArgument, read_geometry},
    numeric_value::finite_or_none,
};
use cassiopeia_geometry::{convert::from_geojson, measure::area, policy::GeometryPolicy, target::GeometryTarget};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `geo_area`: measures the area a `GeoJSON` geometry encloses, in square metres.
///
/// The measurement is geodesic, on the WGS84 ellipsoid RFC 7946 clause 4 fixes the coordinate
/// reference system to, by the method of Karney (2013); a planar measurement would be in square
/// degrees and would mean nothing. A geometry below dimension two encloses no area and measures
/// zero, and a geometry a `GeoProperty` could not hold yields a null so the attribute drops.
///
/// # Errors
/// Returns a [`tera::Error`] when `value` is present but is not a `GeoJSON` geometry.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn geo_area(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let GeometryArgument::Present(geometry) = read_geometry(&kwargs, "geo_area")? else {
        return Ok(Value::none());
    };

    match from_geojson(geometry, GeometryTarget::Preserve, &GeometryPolicy::default()) {
        Ok(admitted) => Ok(finite_or_none(area(&admitted))),
        Err(_refusal) => Ok(Value::none()),
    }
}

#[cfg(test)]
mod tests {
    use crate::template::function::geo_area::geo_area;
    use serde_json::json;
    use tera::{Context, Tera, Value};

    fn number(value: &serde_json::Value) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("geo_area", geo_area);
        tera.add_raw_template("t", "{{ geo_area(value=geometry) }}").unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", Value::try_from_serializable(value).unwrap());

        tera.render("t", &context).unwrap().parse().unwrap()
    }

    #[test]
    fn a_one_degree_square_at_the_equator_measures_roughly_twelve_thousand_square_kilometres() {
        let square = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]],
        });

        let square_kilometres = number(&square) / 1_000_000.0;
        assert!((12_000.0..13_000.0).contains(&square_kilometres), "measured {square_kilometres} km²");
    }

    #[test]
    fn a_curve_encloses_no_area() {
        let line = json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 0.0]]});

        assert!(number(&line).abs() < f64::EPSILON);
    }

    #[test]
    fn a_value_that_is_not_a_geometry_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("geo_area", geo_area);
        tera.add_raw_template("t", "{{ geo_area(value=geometry) }}").unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", Value::from("not a geometry"));

        assert!(tera.render("t", &context).is_err());
    }
}

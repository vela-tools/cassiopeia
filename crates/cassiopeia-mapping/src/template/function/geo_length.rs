use crate::template::{
    function::geometry_argument::{GeometryArgument, read_geometry},
    numeric_value::finite_or_none,
};
use cassiopeia_geometry::{convert::from_geojson, measure::length, policy::GeometryPolicy, target::GeometryTarget};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `geo_length`: measures the length of a `GeoJSON` geometry, in metres.
///
/// The measurement is geodesic, on the WGS84 ellipsoid RFC 7946 clause 4 fixes the coordinate
/// reference system to, by the method of Karney (2013). A curve measures its own length and a
/// surface its perimeter, holes included; a geometry of dimension zero measures nothing.
///
/// # Errors
/// Returns a [`tera::Error`] when `value` is present but is not a `GeoJSON` geometry.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn geo_length(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let GeometryArgument::Present(geometry) = read_geometry(&kwargs, "geo_length")? else {
        return Ok(Value::none());
    };

    match from_geojson(geometry, GeometryTarget::Preserve, &GeometryPolicy::default()) {
        Ok(admitted) => Ok(finite_or_none(length(&admitted))),
        Err(_refusal) => Ok(Value::none()),
    }
}

#[cfg(test)]
mod tests {
    use crate::template::function::geo_length::geo_length;
    use serde_json::json;
    use tera::{Context, Tera, Value};

    fn number(value: &serde_json::Value) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("geo_length", geo_length);
        tera.add_raw_template("t", "{{ geo_length(value=geometry) }}").unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", Value::try_from_serializable(value).unwrap());

        tera.render("t", &context).unwrap().parse().unwrap()
    }

    #[test]
    fn a_degree_of_longitude_at_the_equator_measures_roughly_one_hundred_and_eleven_kilometres() {
        let line = json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 0.0]]});

        let kilometres = number(&line) / 1000.0;
        assert!((111.0..112.0).contains(&kilometres), "measured {kilometres} km");
    }

    #[test]
    fn a_position_has_no_length() {
        assert!(number(&json!({"type": "Point", "coordinates": [1.0, 2.0]})).abs() < f64::EPSILON);
    }

    #[test]
    fn an_absent_value_yields_nothing() {
        let mut tera = Tera::default();
        tera.register_function("geo_length", geo_length);
        tera.add_raw_template("t", "{{ geo_length(value=geometry) }}").unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", Value::none());

        assert_eq!(tera.render("t", &context).unwrap(), "");
    }
}

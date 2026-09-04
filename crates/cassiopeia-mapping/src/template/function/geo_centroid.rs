use crate::template::function::geometry_argument::{GeometryArgument, read_geometry};
use cassiopeia_geometry::{convert::from_geojson, geometry::GeometryKind, policy::GeometryPolicy, strategy::ConversionStrategy, target::GeometryTarget};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `geo_centroid`: takes a `GeoJSON` geometry's centroid as a `Point` geometry.
///
/// The computation is planar, on longitude and latitude read as plane coordinates, so it drifts at
/// high latitude and gives a meaningless result across the antimeridian. It may also fall outside a
/// concave surface; `geo_convert(value=..., to="point", using="point-on-surface")` is the call that
/// cannot.
///
/// A geometry with no coordinate to average yields a null, so the surrounding attribute drops.
///
/// # Errors
/// Returns a [`tera::Error`] when `value` is present but is not a `GeoJSON` geometry.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn geo_centroid(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let GeometryArgument::Present(geometry) = read_geometry(&kwargs, "geo_centroid")? else {
        return Ok(Value::none());
    };
    let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Centroid)).build();

    match from_geojson(geometry, GeometryTarget::Coerce(GeometryKind::Point), &policy) {
        Ok(centroid) => Value::try_from_serializable(&centroid),
        Err(_refusal) => Ok(Value::none()),
    }
}

#[cfg(test)]
mod tests {
    use crate::template::function::geo_centroid::geo_centroid;
    use serde_json::json;
    use tera::{Context, Tera, Value};

    fn render(template: &str, value: &serde_json::Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        tera.register_function("geo_centroid", geo_centroid);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", Value::try_from_serializable(value).unwrap());

        tera.render("t", &context)
    }

    fn square() -> serde_json::Value {
        json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]],
        })
    }

    #[test]
    fn the_centroid_of_a_square_is_its_middle() {
        let rendered = render(r"{% set point = geo_centroid(value=geometry) %}{{ point.coordinates | first }}", &square()).unwrap();

        assert_eq!(rendered, "1.0");
    }

    #[test]
    fn an_absent_value_yields_nothing() {
        assert_eq!(render("{{ geo_centroid(value=geometry) }}", &json!(null)).unwrap(), "");
    }

    #[test]
    fn a_value_that_is_not_a_geometry_is_an_error() {
        assert!(render("{{ geo_centroid(value=geometry) }}", &json!(42)).is_err());
    }
}

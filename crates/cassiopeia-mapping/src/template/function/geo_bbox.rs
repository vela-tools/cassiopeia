use crate::template::function::geometry_argument::{GeometryArgument, read_geometry};
use cassiopeia_geometry::{convert::from_geojson, geometry::GeometryKind, policy::GeometryPolicy, strategy::ConversionStrategy, target::GeometryTarget};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `geo_bbox`: takes a `GeoJSON` geometry's bounding box as a rectangular `Polygon`
/// geometry.
///
/// The rectangle is built in the plane from the geometry's extremes and wound counterclockwise, as
/// RFC 7946 clause 3.1.6 requires of an exterior ring. It is available from every geometry,
/// including a lone position, whose bounding box is a structurally valid rectangle of zero area.
///
/// # Errors
/// Returns a [`tera::Error`] when `value` is present but is not a `GeoJSON` geometry.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn geo_bbox(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let GeometryArgument::Present(geometry) = read_geometry(&kwargs, "geo_bbox")? else {
        return Ok(Value::none());
    };
    let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Envelope)).build();

    match from_geojson(geometry, GeometryTarget::Coerce(GeometryKind::Polygon), &policy) {
        Ok(envelope) => Value::try_from_serializable(&envelope),
        Err(_refusal) => Ok(Value::none()),
    }
}

#[cfg(test)]
mod tests {
    use crate::template::function::geo_bbox::geo_bbox;
    use serde_json::json;
    use tera::{Context, Tera, Value};

    fn render(template: &str, value: &serde_json::Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        tera.register_function("geo_bbox", geo_bbox);
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
    fn the_bounding_box_of_a_square_is_a_closed_rectangle() {
        let rendered = render(
            r"{% set box = geo_bbox(value=geometry) %}{{ box.type }} {{ box.coordinates | first | length }}",
            &square(),
        )
        .unwrap();

        assert_eq!(rendered, "Polygon 5");
    }

    #[test]
    fn a_lone_position_bounds_a_rectangle_of_zero_area() {
        let rendered = render(
            r"{% set box = geo_bbox(value=geometry) %}{{ box.type }} {{ box.coordinates | first | length }}",
            &json!({"type": "Point", "coordinates": [1.0, 2.0]}),
        )
        .unwrap();

        assert_eq!(rendered, "Polygon 5");
    }

    #[test]
    fn an_absent_value_yields_nothing() {
        assert_eq!(render("{{ geo_bbox(value=geometry) }}", &json!(null)).unwrap(), "");
    }
}

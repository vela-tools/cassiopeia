use crate::template::function::geometry_argument::{GeometryArgument, read_geometry, read_strategy, read_target};
use cassiopeia_geometry::{convert::from_geojson, lattice::check, policy::GeometryPolicy};
use tera::{Error, Kwargs, State, TeraResult, Value};

/// Tera function `geo_convert`: converts a `GeoJSON` geometry to another geometry type, for a
/// geometry that has to be embedded in a structure an attribute's `transformation` cannot reach.
///
/// `value` is the source geometry, `to` names the geometry type to produce in the same vocabulary an
/// attribute's `transformation` uses (`point`, `multipolygon`, `geometry`), and the optional `using`
/// names the conversion to apply, in the same vocabulary an attribute's `geometry: { convert: ... }`
/// uses. Without `using`, only the lossless conversions are available, exactly as in a mapping.
///
/// A conversion the source cannot satisfy yields a null, so the surrounding attribute drops and the
/// record survives; a misconfigured call (an unknown type or conversion, a conversion that can never
/// produce the requested type, a `value` that is not a geometry) is an error, because no record
/// could satisfy it.
///
/// # Errors
/// Returns a [`tera::Error`] for a misconfigured call.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn geo_convert(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let GeometryArgument::Present(geometry) = read_geometry(&kwargs, "geo_convert")? else {
        return Ok(Value::none());
    };
    let target = read_target(&kwargs, "geo_convert")?;
    let strategy = read_strategy(&kwargs, "geo_convert")?;
    check(target, strategy).map_err(|refusal| Error::message(format!("Function `geo_convert` cannot run this conversion: {refusal}")))?;

    let policy = GeometryPolicy::builder().convert(strategy).build();
    match from_geojson(geometry, target, &policy) {
        Ok(converted) => Value::try_from_serializable(&converted),
        Err(_refusal) => Ok(Value::none()),
    }
}

#[cfg(test)]
mod tests {
    use crate::template::function::geo_convert::geo_convert;
    use serde_json::json;
    use tera::{Context, Tera, Value};

    fn render(template: &str, value: &serde_json::Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        tera.register_function("geo_convert", geo_convert);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        context.insert_value("geometry", Value::try_from_serializable(value).unwrap());

        tera.render("t", &context)
    }

    fn multi_polygon() -> serde_json::Value {
        json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[10.0, 10.0], [13.0, 10.0], [13.0, 13.0], [10.0, 10.0]]],
            ],
        })
    }

    #[test]
    fn a_declared_conversion_produces_the_requested_type() {
        let rendered = render(
            r#"{% set converted = geo_convert(value=geometry, to="polygon", using="largest") %}{{ converted.type }}"#,
            &multi_polygon(),
        )
        .unwrap();

        assert_eq!(rendered, "Polygon");
    }

    #[test]
    fn a_lossy_conversion_without_a_declaration_yields_nothing() {
        assert_eq!(render(r#"{{ geo_convert(value=geometry, to="polygon") }}"#, &multi_polygon()).unwrap(), "");
    }

    #[test]
    fn a_conversion_that_can_never_produce_the_requested_type_is_an_error() {
        assert!(render(r#"{{ geo_convert(value=geometry, to="point", using="largest") }}"#, &multi_polygon()).is_err());
    }

    #[test]
    fn an_unknown_conversion_is_an_error() {
        assert!(render(r#"{{ geo_convert(value=geometry, to="point", using="buffer") }}"#, &multi_polygon()).is_err());
    }

    #[test]
    fn an_unknown_target_type_is_an_error() {
        assert!(render(r#"{{ geo_convert(value=geometry, to="geohash") }}"#, &multi_polygon()).is_err());
    }

    #[test]
    fn a_value_that_is_not_a_geometry_is_an_error() {
        assert!(render(r#"{{ geo_convert(value=geometry, to="point") }}"#, &json!("not a geometry")).is_err());
    }

    #[test]
    fn an_absent_value_yields_nothing() {
        assert_eq!(render(r#"{{ geo_convert(value=geometry, to="point") }}"#, &json!(null)).unwrap(), "");
    }
}

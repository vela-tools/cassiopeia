use crate::template::numeric_value::{finite_or_none, number_from_value};
use tera::{Kwargs, State, Tera, TeraResult, Value};

/// Registers the directional rounding filters on a Tera engine.
///
/// These sit alongside the built-in `round(method=...)`: `round` picks the nearest integer (or
/// rounds up/down through its `method` argument), while these three name a fixed direction, which
/// reads more clearly at a mapping's call site.
///
/// | filter | direction |
/// |---|---|
/// | `floor` | toward negative infinity |
/// | `ceil` | toward positive infinity |
/// | `trunc` | toward zero (drop the fraction) |
pub fn register(tera: &mut Tera) {
    tera.register_filter("floor", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "floor")?.floor()))
    });
    tera.register_filter("ceil", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "ceil")?.ceil()))
    });
    tera.register_filter("trunc", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "trunc")?.trunc()))
    });
}

#[cfg(test)]
mod tests {
    use crate::template::filter::rounding::register;
    use tera::{Context, Tera, Value};

    fn apply(template: &str, value: Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        context.insert_value("value", value);

        tera.render("t", &context)
    }

    fn number(template: &str, value: Value) -> f64 {
        apply(template, value).unwrap().parse().unwrap()
    }

    #[test]
    fn floor_rounds_a_positive_down() {
        assert!((number("{{ value | floor }}", Value::from(2.9)) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn floor_rounds_a_negative_toward_negative_infinity() {
        assert!((number("{{ value | floor }}", Value::from(-2.1)) + 3.0).abs() < 1e-9);
    }

    #[test]
    fn ceil_rounds_a_positive_up() {
        assert!((number("{{ value | ceil }}", Value::from(2.1)) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn trunc_drops_the_fraction_of_a_negative() {
        assert!((number("{{ value | trunc }}", Value::from(-2.9)) + 2.0).abs() < 1e-9);
    }

    #[test]
    fn trunc_reads_a_value_written_as_a_string() {
        assert!((number("{{ value | trunc }}", Value::from("2.9")) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_non_numeric_value_is_an_error() {
        assert!(apply("{{ value | floor }}", Value::from("north")).is_err());
    }
}

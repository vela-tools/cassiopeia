use crate::template::numeric_value::{finite_or_none, number_from_value};
use tera::{Kwargs, State, Tera, TeraResult, Value};

/// Registers the elementary unary math filters on a Tera engine.
///
/// Each filter reads its piped value as a number (tolerating the numeric-string form text sources
/// carry) and applies one `f64` operation. A domain-invalid result (`sqrt` of a negative, `ln` of
/// a non-positive number) collapses to a null so the attribute drops and the entity survives,
/// rather than failing the whole record.
///
/// | filter | operation |
/// |---|---|
/// | `sqrt` | square root |
/// | `cbrt` | cube root |
/// | `sign` | -1, 0, or 1 by sign |
/// | `exp` | e raised to the value |
/// | `ln` | natural logarithm |
/// | `log10` | base-10 logarithm |
/// | `log2` | base-2 logarithm |
pub fn register(tera: &mut Tera) {
    tera.register_filter("sqrt", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "sqrt")?.sqrt()))
    });
    tera.register_filter("cbrt", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "cbrt")?.cbrt()))
    });
    tera.register_filter("sign", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(sign(number_from_value(value, "sign")?)))
    });
    tera.register_filter("exp", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "exp")?.exp()))
    });
    tera.register_filter("ln", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "ln")?.ln()))
    });
    tera.register_filter("log10", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "log10")?.log10()))
    });
    tera.register_filter("log2", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "log2")?.log2()))
    });
}

/// Reports the sign of a number as -1, 0, or 1.
///
/// Unlike [`f64::signum`], which returns 1 for a positive zero and -1 for a negative zero, a zero
/// input maps to a zero here: the sign convention a mapping expects.
fn sign(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value.signum() }
}

#[cfg(test)]
mod tests {
    use crate::template::filter::elementary::register;
    use std::f64::consts::E;
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
    fn sqrt_of_a_perfect_square_is_the_root() {
        assert!((number("{{ value | sqrt }}", Value::from(16.0)) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn sqrt_reads_a_value_written_as_a_string() {
        assert!((number("{{ value | sqrt }}", Value::from("16")) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn sqrt_of_a_negative_renders_empty() {
        assert_eq!(apply("{{ value | sqrt }}", Value::from(-1.0)).unwrap(), "");
    }

    #[test]
    fn cbrt_of_a_cube_is_the_root() {
        assert!((number("{{ value | cbrt }}", Value::from(27.0)) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn sign_of_a_negative_is_minus_one() {
        assert!((number("{{ value | sign }}", Value::from(-4.2)) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn sign_of_zero_is_zero() {
        assert!(number("{{ value | sign }}", Value::from(0.0)).abs() < 1e-9);
    }

    #[test]
    fn ln_of_e_is_one() {
        assert!((number("{{ value | ln }}", Value::from(E)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn ln_of_zero_renders_empty() {
        assert_eq!(apply("{{ value | ln }}", Value::from(0.0)).unwrap(), "");
    }

    #[test]
    fn log10_of_a_thousand_is_three() {
        assert!((number("{{ value | log10 }}", Value::from(1000.0)) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn log2_of_eight_is_three() {
        assert!((number("{{ value | log2 }}", Value::from(8.0)) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn exp_of_zero_is_one() {
        assert!((number("{{ value | exp }}", Value::from(0.0)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_non_numeric_value_is_an_error() {
        assert!(apply("{{ value | sqrt }}", Value::from("north")).is_err());
    }
}

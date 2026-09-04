use crate::template::numeric_value::{finite_or_none, number_from_value};
use tera::{Kwargs, State, Tera, TeraResult, Value};

/// Registers the trigonometric filters and the radian/degree conversions on a Tera engine.
///
/// The circular functions take and return radians, matching `f64`; `radians` and `degrees` convert
/// between the two so a mapping can work in whichever unit its source uses. The inverse functions
/// `asin` and `acos` are only defined on `[-1, 1]`; an argument outside that range collapses to a
/// null (dropping the attribute) rather than failing the record.
///
/// | filter | operation |
/// |---|---|
/// | `sin`, `cos`, `tan` | circular functions of an angle in radians |
/// | `asin`, `acos`, `atan` | inverse circular functions, returning radians |
/// | `radians` | degrees to radians |
/// | `degrees` | radians to degrees |
pub fn register(tera: &mut Tera) {
    tera.register_filter("sin", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "sin")?.sin()))
    });
    tera.register_filter("cos", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "cos")?.cos()))
    });
    tera.register_filter("tan", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "tan")?.tan()))
    });
    tera.register_filter("asin", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "asin")?.asin()))
    });
    tera.register_filter("acos", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "acos")?.acos()))
    });
    tera.register_filter("atan", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "atan")?.atan()))
    });
    tera.register_filter("radians", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "radians")?.to_radians()))
    });
    tera.register_filter("degrees", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        Ok(finite_or_none(number_from_value(value, "degrees")?.to_degrees()))
    });
}

#[cfg(test)]
mod tests {
    use crate::template::filter::trigonometry::register;
    use std::f64::consts::PI;
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
    fn sin_of_zero_is_zero() {
        assert!(number("{{ value | sin }}", Value::from(0.0)).abs() < 1e-9);
    }

    #[test]
    fn cos_of_zero_is_one() {
        assert!((number("{{ value | cos }}", Value::from(0.0)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sin_of_half_pi_is_one() {
        assert!((number("{{ value | sin }}", Value::from(PI / 2.0)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn radians_of_a_hundred_and_eighty_degrees_is_pi() {
        assert!((number("{{ value | radians }}", Value::from(180.0)) - PI).abs() < 1e-9);
    }

    #[test]
    fn degrees_of_pi_is_a_hundred_and_eighty() {
        assert!((number("{{ value | degrees }}", Value::from(PI)) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn asin_of_one_is_half_pi() {
        assert!((number("{{ value | asin }}", Value::from(1.0)) - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn asin_outside_the_domain_renders_empty() {
        assert_eq!(apply("{{ value | asin }}", Value::from(2.0)).unwrap(), "");
    }

    #[test]
    fn radians_reads_a_value_written_as_a_string() {
        assert!((number("{{ value | radians }}", Value::from("180")) - PI).abs() < 1e-9);
    }

    #[test]
    fn a_non_numeric_value_is_an_error() {
        assert!(apply("{{ value | sin }}", Value::from("north")).is_err());
    }
}

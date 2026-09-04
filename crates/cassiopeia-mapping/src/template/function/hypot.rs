use crate::template::numeric_value::{finite_or_none, number_arg};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `hypot`: the magnitude of a two-component vector, `sqrt(x^2 + y^2)`.
///
/// This is the universal vector-length helper the derived-quantity aliases build on: wind speed
/// from its east/north components, current speed, wave energy: anything stored as a pair of
/// orthogonal components. It uses [`f64::hypot`], which avoids the overflow a naive
/// `sqrt(x*x + y*y)` risks for large components. Either component may be written as a number or as a
/// numeric string.
///
/// # Errors
/// Returns a [`tera::Error`] when `x` or `y` is missing or is not readable as a number. A
/// non-finite magnitude resolves to a null so the attribute is dropped rather than the record lost.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn hypot(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let x = number_arg(&kwargs, "x")?;
    let y = number_arg(&kwargs, "y")?;

    Ok(finite_or_none(x.hypot(y)))
}

#[cfg(test)]
mod tests {
    use crate::template::function::hypot::hypot;
    use tera::{Context, Tera};

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("hypot", hypot);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn the_magnitude_of_a_three_four_vector_is_five() {
        assert!((number("{{ hypot(x=3, y=4) }}") - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_zero_vector_has_zero_magnitude() {
        assert!(number("{{ hypot(x=0, y=0) }}").abs() < 1e-9);
    }

    #[test]
    fn components_written_as_strings_are_read() {
        assert!((number(r#"{{ hypot(x="3", y="4") }}"#) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_missing_component_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("hypot", hypot);
        tera.add_raw_template("t", "{{ hypot(x=3) }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

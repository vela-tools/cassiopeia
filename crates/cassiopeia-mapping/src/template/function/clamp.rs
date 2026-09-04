use crate::template::numeric_value::{finite_or_none, number_arg};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `clamp`: constrains `value` to the closed interval `[min, max]`.
///
/// A value below `min` becomes `min`, a value above `max` becomes `max`, and a value already inside
/// the interval is unchanged. Useful for holding a derived quantity inside the range its Smart Data
/// Model permits: a ratio to `[0, 1]`, a percentage to `[0, 100]`. Every argument may be written as
/// a number or a numeric string.
///
/// It is implemented with `max` then `min` rather than [`f64::clamp`], which panics when `min` is
/// greater than `max`; here that misordering is tolerated and yields `max`, never a panic.
///
/// # Errors
/// Returns a [`tera::Error`] when any of `value`, `min`, or `max` is missing or not readable as a
/// number.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn clamp(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let value = number_arg(&kwargs, "value")?;
    let min = number_arg(&kwargs, "min")?;
    let max = number_arg(&kwargs, "max")?;

    Ok(finite_or_none(value.max(min).min(max)))
}

#[cfg(test)]
mod tests {
    use crate::template::function::clamp::clamp;
    use tera::{Context, Tera};

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("clamp", clamp);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn a_value_inside_the_interval_is_unchanged() {
        assert!((number("{{ clamp(value=0.5, min=0, max=1) }}") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_value_below_the_minimum_is_raised_to_it() {
        assert!((number("{{ clamp(value=-2, min=0, max=1) }}")).abs() < 1e-9);
    }

    #[test]
    fn a_value_above_the_maximum_is_lowered_to_it() {
        assert!((number("{{ clamp(value=5, min=0, max=1) }}") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn arguments_written_as_strings_are_read() {
        assert!((number(r#"{{ clamp(value="5", min="0", max="1") }}"#) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_missing_bound_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("clamp", clamp);
        tera.add_raw_template("t", "{{ clamp(value=5, min=0) }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

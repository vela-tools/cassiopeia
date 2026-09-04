use crate::template::numeric_value::{finite_or_none, number_arg};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `atan2`: the angle in radians between the positive x-axis and the point `(x, y)`,
/// in the full `(-pi, pi]` range.
///
/// Unlike a bare `atan(y / x)`, it uses the signs of both components to place the angle in the
/// correct quadrant, and is defined when `x` is zero. It is the primitive the compass `bearing`
/// helper builds on; exposed directly for any other quadrant-aware angle a mapping needs. Either
/// argument may be written as a number or a numeric string.
///
/// # Errors
/// Returns a [`tera::Error`] when `y` or `x` is missing or not readable as a number.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn atan2(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let y = number_arg(&kwargs, "y")?;
    let x = number_arg(&kwargs, "x")?;

    Ok(finite_or_none(y.atan2(x)))
}

#[cfg(test)]
mod tests {
    use crate::template::function::atan2::atan2;
    use std::f64::consts::PI;
    use tera::{Context, Tera};

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("atan2", atan2);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn the_positive_y_axis_is_a_quarter_turn() {
        assert!((number("{{ atan2(y=1, x=0) }}") - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_negative_x_axis_is_a_half_turn() {
        assert!((number("{{ atan2(y=0, x=-1) }}") - PI).abs() < 1e-9);
    }

    #[test]
    fn the_origin_direction_is_zero() {
        assert!(number("{{ atan2(y=0, x=0) }}").abs() < 1e-9);
    }

    #[test]
    fn arguments_written_as_strings_are_read() {
        assert!((number(r#"{{ atan2(y="1", x="0") }}"#) - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_missing_argument_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("atan2", atan2);
        tera.add_raw_template("t", "{{ atan2(y=1) }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

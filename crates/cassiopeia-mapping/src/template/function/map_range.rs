use crate::template::numeric_value::{finite_or_none, number_arg};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `map_range`: linearly rescales `value` from the input span `[in_min, in_max]` onto
/// the output span `[out_min, out_max]`.
///
/// The endpoints map to the endpoints and everything between scales proportionally, so a raw
/// sensor reading on one scale becomes the quantity a Smart Data Model expects on another. The
/// mapping is not clamped: an input outside the input span extrapolates past the output span. Every
/// argument may be written as a number or a numeric string.
///
/// A zero-width input span (`in_min == in_max`) has no defined slope; the result is non-finite and
/// resolves to a null, dropping the attribute rather than failing the record.
///
/// # Errors
/// Returns a [`tera::Error`] when any of the five arguments is missing or not readable as a number.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn map_range(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let value = number_arg(&kwargs, "value")?;
    let in_min = number_arg(&kwargs, "in_min")?;
    let in_max = number_arg(&kwargs, "in_max")?;
    let out_min = number_arg(&kwargs, "out_min")?;
    let out_max = number_arg(&kwargs, "out_max")?;

    let fraction = (value - in_min) / (in_max - in_min);

    Ok(finite_or_none(out_min + fraction * (out_max - out_min)))
}

#[cfg(test)]
mod tests {
    use crate::template::function::map_range::map_range;
    use tera::{Context, Tera};

    fn render(template: &str) -> String {
        let mut tera = Tera::default();
        tera.register_function("map_range", map_range);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap()
    }

    fn number(template: &str) -> f64 {
        render(template).parse().unwrap()
    }

    #[test]
    fn the_input_minimum_maps_to_the_output_minimum() {
        assert!((number("{{ map_range(value=0, in_min=0, in_max=10, out_min=0, out_max=100) }}")).abs() < 1e-9);
    }

    #[test]
    fn the_input_maximum_maps_to_the_output_maximum() {
        assert!((number("{{ map_range(value=10, in_min=0, in_max=10, out_min=0, out_max=100) }}") - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_midpoint_maps_to_the_output_midpoint() {
        assert!((number("{{ map_range(value=5, in_min=0, in_max=10, out_min=0, out_max=100) }}") - 50.0).abs() < 1e-9);
    }

    #[test]
    fn arguments_written_as_strings_are_read() {
        assert!((number(r#"{{ map_range(value="5", in_min="0", in_max="10", out_min="0", out_max="100") }}"#) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_zero_width_input_span_renders_empty() {
        assert_eq!(render("{{ map_range(value=5, in_min=2, in_max=2, out_min=0, out_max=100) }}"), "");
    }

    #[test]
    fn a_missing_argument_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("map_range", map_range);
        tera.add_raw_template("t", "{{ map_range(value=5, in_min=0, in_max=10, out_min=0) }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

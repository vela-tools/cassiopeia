use crate::template::{
    function::bearing::{Convention, compass_bearing},
    numeric_value::{finite_or_none, number_arg},
};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `wind_speed`: the wind speed from its `u` (eastward) and `v` (northward)
/// components, `sqrt(u^2 + v^2)`.
///
/// This is a named alias of the universal `hypot` helper, for the common case of a gridded weather
/// product (GRIB and the like) that stores wind as orthogonal components rather than as a
/// speed/direction pair. The result carries the components' unit: metres per second for the usual
/// `10u`/`10v` fields. Either component may be written as a number or a numeric string.
///
/// # Errors
/// Returns a [`tera::Error`] when `u` or `v` is missing or not readable as a number.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn wind_speed(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let u = number_arg(&kwargs, "u")?;
    let v = number_arg(&kwargs, "v")?;

    Ok(finite_or_none(u.hypot(v)))
}

/// Tera function `wind_direction`: the meteorological wind direction in degrees from its `u`
/// (eastward) and `v` (northward) components: the compass bearing the wind blows *from*.
///
/// This is a named alias of the universal `bearing` helper under its `from` convention. A wind
/// whose `u` is positive (blowing toward the east) reads 270 degrees, a wind blowing toward the
/// north reads 180, the direction it originates. Either component may be written as a number or a
/// numeric string.
///
/// # Errors
/// Returns a [`tera::Error`] when `u` or `v` is missing or not readable as a number.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn wind_direction(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let u = number_arg(&kwargs, "u")?;
    let v = number_arg(&kwargs, "v")?;

    Ok(finite_or_none(compass_bearing(u, v, Convention::From)))
}

#[cfg(test)]
mod tests {
    use crate::template::function::wind::{wind_direction, wind_speed};
    use tera::{Context, Tera};

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("wind_speed", wind_speed);
        tera.register_function("wind_direction", wind_direction);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn wind_speed_is_the_magnitude_of_the_components() {
        assert!((number("{{ wind_speed(u=3, v=4) }}") - 5.0).abs() < 1e-9);
    }

    #[test]
    fn an_eastward_wind_blows_from_the_west() {
        assert!((number("{{ wind_direction(u=1, v=0) }}") - 270.0).abs() < 1e-9);
    }

    #[test]
    fn a_northward_wind_blows_from_the_south() {
        assert!((number("{{ wind_direction(u=0, v=1) }}") - 180.0).abs() < 1e-9);
    }

    #[test]
    fn components_written_as_strings_are_read() {
        assert!((number(r#"{{ wind_speed(u="3", v="4") }}"#) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_missing_component_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("wind_speed", wind_speed);
        tera.add_raw_template("t", "{{ wind_speed(u=3) }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

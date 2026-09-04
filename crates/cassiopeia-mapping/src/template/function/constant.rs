use std::f64::consts::{E, PI, TAU};
use tera::{Kwargs, State, TeraResult, Value};

/// Tera function `pi`: the mathematical constant pi, the ratio of a circle's circumference to its
/// diameter.
///
/// # Errors
/// Never fails; the `TeraResult` return matches the other math functions so registration is
/// uniform.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn pi(_kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    Ok(Value::from(PI))
}

/// Tera function `tau`: the mathematical constant tau, two times pi, the radians in a full turn.
///
/// # Errors
/// Never fails; the `TeraResult` return matches the other math functions so registration is
/// uniform.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn tau(_kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    Ok(Value::from(TAU))
}

/// Tera function `e`: Euler's number, the base of the natural logarithm.
///
/// # Errors
/// Never fails; the `TeraResult` return matches the other math functions so registration is
/// uniform.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn e(_kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    Ok(Value::from(E))
}

#[cfg(test)]
mod tests {
    use crate::template::function::constant::{e, pi, tau};
    use std::f64::consts::{E, PI, TAU};
    use tera::{Context, Tera};

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("pi", pi);
        tera.register_function("tau", tau);
        tera.register_function("e", e);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn pi_renders_the_constant() {
        assert!((number("{{ pi() }}") - PI).abs() < 1e-12);
    }

    #[test]
    fn tau_renders_two_pi() {
        assert!((number("{{ tau() }}") - TAU).abs() < 1e-12);
    }

    #[test]
    fn e_renders_eulers_number() {
        assert!((number("{{ e() }}") - E).abs() < 1e-12);
    }
}

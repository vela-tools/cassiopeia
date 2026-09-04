use crate::template::numeric_value::{finite_or_none, number_arg};
use strum::EnumString;
use tera::{Error, Kwargs, State, TeraResult, Value};

/// Which direction a compass bearing names for a vector given by its east/north components.
///
/// The distinction is the difference between a meteorological and an oceanographic convention:
/// wind is reported by the direction it comes *from*, while a current or a vehicle is reported by
/// the direction it goes *to*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, EnumString)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum Convention {
    /// The direction the vector points away from: the meteorological convention for wind.
    #[default]
    From,
    /// The direction the vector points toward: the oceanographic convention for currents.
    To,
}

/// The compass bearing in degrees clockwise from north for a vector given by its `east` and `north`
/// components, under the chosen [`Convention`].
///
/// The `to` bearing is `atan2(east, north)` folded into `[0, 360)`; the `from` bearing is that
/// turned by 180 degrees. So a vector pointing due east (`east > 0`, `north == 0`) reads 90 `to`
/// and 270 `from`, and one pointing due north reads 0 `to` and 180 `from`.
pub(crate) fn compass_bearing(east: f64, north: f64, convention: Convention) -> f64 {
    let toward = east.atan2(north).to_degrees().rem_euclid(360.0);

    match convention {
        Convention::To => toward,
        Convention::From => (toward + 180.0).rem_euclid(360.0),
    }
}

/// Tera function `bearing`: the compass bearing in degrees clockwise from north of a vector given
/// by its `east` and `north` components.
///
/// `convention` chooses which direction the bearing names: `"from"` (the default, the
/// meteorological convention: where the vector comes from) or `"to"` (where it points). This is the
/// universal direction helper the `wind_direction` alias builds on; it applies equally to a current
/// or any other east/north vector. `east` and `north` may each be written as a number or a numeric
/// string.
///
/// # Errors
/// Returns a [`tera::Error`] when `east` or `north` is missing or unreadable, or when `convention`
/// is present but is neither `from` nor `to`.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn bearing(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let east = number_arg(&kwargs, "east")?;
    let north = number_arg(&kwargs, "north")?;
    let convention = match kwargs.get::<String>("convention")? {
        Some(text) => text
            .parse()
            .map_err(|_| Error::message(format!("`convention` must be `from` or `to`, got `{text}`")))?,
        None => Convention::default(),
    };

    Ok(finite_or_none(compass_bearing(east, north, convention)))
}

#[cfg(test)]
mod tests {
    use crate::template::function::bearing::bearing;
    use tera::{Context, Tera};

    fn number(template: &str) -> f64 {
        let mut tera = Tera::default();
        tera.register_function("bearing", bearing);
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap().parse().unwrap()
    }

    #[test]
    fn a_vector_pointing_east_comes_from_the_west() {
        assert!((number("{{ bearing(east=1, north=0) }}") - 270.0).abs() < 1e-9);
    }

    #[test]
    fn a_vector_pointing_north_comes_from_the_south() {
        assert!((number("{{ bearing(east=0, north=1) }}") - 180.0).abs() < 1e-9);
    }

    #[test]
    fn the_to_convention_names_where_the_vector_points() {
        assert!((number(r#"{{ bearing(east=1, north=0, convention="to") }}"#) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn components_written_as_strings_are_read() {
        assert!((number(r#"{{ bearing(east="1", north="0") }}"#) - 270.0).abs() < 1e-9);
    }

    #[test]
    fn an_unknown_convention_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("bearing", bearing);
        tera.add_raw_template("t", r#"{{ bearing(east=1, north=0, convention="sideways") }}"#).unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }

    #[test]
    fn a_missing_component_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("bearing", bearing);
        tera.add_raw_template("t", "{{ bearing(east=1) }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

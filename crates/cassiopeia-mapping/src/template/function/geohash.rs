use crate::template::numeric_value::number_from_value;
use geohash::{Coord, encode};
use tera::{Error, Kwargs, State, TeraResult, Value};

/// Default geohash length when a mapping does not pass `precision`.
///
/// Nine characters resolve to roughly a five-metre cell, so two records that carry genuinely
/// different coordinates receive different codes. Same-named places within one country sit far
/// enough apart that this keeps their entity ids distinct.
const DEFAULT_PRECISION: usize = 9;

/// Tera function `geohash`: encodes a `lat`/`lon` pair into a short, stable geohash string of
/// `precision` characters (default nine).
///
/// A geohash is a deterministic, URN-safe code drawn from `[0-9b-hjkmnp-z]`, so it composes into an
/// `entityName` without any further cleaning and gives same-named records a location-derived
/// suffix that keeps their entity ids apart. Both coordinates may be written as numbers or as the
/// numeric strings open datasets commonly use.
///
/// # Errors
/// Returns a [`tera::Error`] when either coordinate is missing or unreadable, when `precision` is
/// not a whole number, or when the geohash crate rejects the coordinate or the requested length.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn geohash(kwargs: Kwargs, _state: &State) -> TeraResult<String> {
    let latitude = coordinate(kwargs.get::<Value>("lat")?, "lat")?;
    let longitude = coordinate(kwargs.get::<Value>("lon")?, "lon")?;
    let precision = precision_argument(kwargs.get::<Value>("precision")?)?;

    encode(Coord { x: longitude, y: latitude }, precision)
        .map_err(|error| Error::message(format!("Function `geohash` could not encode ({latitude}, {longitude}): {error}")))
}

/// Reads one coordinate argument, which a mapping may write either as a number or as a string.
fn coordinate(argument: Option<Value>, name: &str) -> TeraResult<f64> {
    let Some(argument) = argument else {
        return Err(Error::message(format!("Function `geohash` needs a `{name}` argument")));
    };

    number_from_value(&argument, name)
}

/// Reads the optional `precision` argument, defaulting to [`DEFAULT_PRECISION`].
fn precision_argument(argument: Option<Value>) -> TeraResult<usize> {
    let Some(argument) = argument else {
        return Ok(DEFAULT_PRECISION);
    };

    if let Some(number) = argument.as_u64() {
        return usize::try_from(number).map_err(|_| Error::message(format!("Function `geohash` precision `{number}` is out of range")));
    }

    if let Some(text) = argument.as_str() {
        return text
            .parse()
            .map_err(|_| Error::message(format!("Function `geohash` could not read precision `{text}` as a whole number")));
    }

    Err(Error::message(format!(
        "Function `geohash` needs a whole number for `precision`, got {argument}"
    )))
}

#[cfg(test)]
mod tests {
    use crate::template::function::geohash::geohash;
    use tera::{Context, Tera, Value};

    fn render(template: &str, latitude: Value, longitude: Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        tera.register_function("geohash", geohash);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        context.insert_value("lat", latitude);
        context.insert_value("lon", longitude);

        tera.render("t", &context)
    }

    #[test]
    fn encodes_a_numeric_coordinate_pair() {
        assert_eq!(
            render("{{ geohash(lat=lat, lon=lon) }}", Value::from(35.3003), Value::from(-120.6623)).unwrap(),
            "9q60y60rh"
        );
    }

    #[test]
    fn reads_coordinates_written_as_strings() {
        assert_eq!(
            render("{{ geohash(lat=lat, lon=lon) }}", Value::from("42.50729"), Value::from("1.53414")).unwrap(),
            render("{{ geohash(lat=lat, lon=lon) }}", Value::from(42.50729), Value::from(1.53414)).unwrap()
        );
    }

    #[test]
    fn honours_an_explicit_precision() {
        assert_eq!(
            render("{{ geohash(lat=lat, lon=lon, precision=5) }}", Value::from(35.3003), Value::from(-120.6623)).unwrap(),
            "9q60y"
        );
    }

    #[test]
    fn defaults_to_a_nine_character_code() {
        let code = render("{{ geohash(lat=lat, lon=lon) }}", Value::from(35.3003), Value::from(-120.6623)).unwrap();

        assert_eq!(code.len(), 9);
    }

    #[test]
    fn distinct_coordinates_yield_distinct_codes() {
        let first = render("{{ geohash(lat=lat, lon=lon) }}", Value::from(53.55073), Value::from(9.99302)).unwrap();
        let second = render("{{ geohash(lat=lat, lon=lon) }}", Value::from(42.50729), Value::from(1.53414)).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn a_missing_coordinate_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("geohash", geohash);
        tera.add_raw_template("t", "{{ geohash(lat=lat) }}").unwrap();

        let mut context = Context::new();
        context.insert("lat", &35.3003);

        assert!(tera.render("t", &context).is_err());
    }

    #[test]
    fn an_unreadable_coordinate_is_an_error() {
        assert!(render("{{ geohash(lat=lat, lon=lon) }}", Value::from("north"), Value::from(1.5)).is_err());
    }

    #[test]
    fn an_out_of_range_latitude_is_an_error() {
        assert!(render("{{ geohash(lat=lat, lon=lon) }}", Value::from(200.0), Value::from(1.5)).is_err());
    }
}

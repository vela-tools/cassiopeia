use lazy_regex::regex;
use serde_json::json;
use tera::{Error, Kwargs, State, TeraResult, Value};

/// One parsed sexagesimal coordinate: its magnitude in decimal degrees and the hemisphere letter
/// that fixes its sign and its axis.
struct Sexagesimal {
    degrees: f64,
    hemisphere: char,
}

/// Tera function `dms_point`: parses a labelled degrees-minutes-seconds coordinate pair into a
/// `GeoJSON` `Point` string, ready for a `GeoProperty` under the `point` transformation.
///
/// The `value` argument is a single string carrying both axes with hemisphere markers, as gazetteers
/// commonly print them: `27°59′17″N 86°55′30″E`. Degree, minute, and second markers may be written
/// with the typographic primes (`′`, `″`) or the ASCII apostrophe and quote; the minute and second
/// fields are optional, and any trailing footnote marker after the hemisphere letter is ignored. The
/// two axes are read in either order and combined into the RFC 7946 `[longitude, latitude]` ordering
/// (clause 3.1.1), so the north/south reading always lands in the second slot regardless of which
/// token came first.
///
/// # Errors
/// Returns a [`tera::Error`] when `value` is missing or not a string, when the string does not hold
/// exactly one north/south reading and one east/west reading, or when a numeric field cannot be read.
// `Kwargs` is taken by value because tera's blanket `Function` impl is `Fn(Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_function` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Function trait requires Kwargs by value")]
pub fn dms_point(kwargs: Kwargs, _state: &State) -> TeraResult<String> {
    let Some(argument) = kwargs.get::<Value>("value")? else {
        return Err(Error::message("Function `dms_point` needs a `value` argument".to_string()));
    };
    let Some(text) = argument.as_str() else {
        return Err(Error::message(format!("Function `dms_point` needs a string for `value`, got {argument}")));
    };

    let (longitude, latitude) = parse_pair(text)?;
    Ok(json!({ "type": "Point", "coordinates": [longitude, latitude] }).to_string())
}

/// Parses a coordinate pair into `(longitude, latitude)` decimal degrees.
fn parse_pair(text: &str) -> TeraResult<(f64, f64)> {
    let mut longitude = None;
    let mut latitude = None;

    for reading in readings(text)? {
        let signed = signed_degrees(&reading);
        match reading.hemisphere {
            'N' | 'S' => set_once(&mut latitude, signed, "north/south", text)?,
            'E' | 'W' => set_once(&mut longitude, signed, "east/west", text)?,
            other => {
                return Err(Error::message(format!(
                    "Function `dms_point` found an unknown hemisphere `{other}` in `{text}`"
                )));
            }
        }
    }

    match (longitude, latitude) {
        (Some(longitude), Some(latitude)) => Ok((longitude, latitude)),
        (_, _) => Err(Error::message(format!(
            "Function `dms_point` needs one north/south and one east/west reading in `{text}`"
        ))),
    }
}

/// Extracts every degrees-minutes-seconds reading the string carries.
fn readings(text: &str) -> TeraResult<Vec<Sexagesimal>> {
    // Degrees are required; the minute (′ or ') and second (″ or ") fields are each optional, and the
    // hemisphere letter anchors the match so any trailing footnote marker after it is left behind.
    let pattern = regex!(r#"(?i)(\d+(?:\.\d+)?)°(?:\s*(\d+(?:\.\d+)?)['′’])?(?:\s*(\d+(?:\.\d+)?)["″”])?\s*([NSEW])"#);

    pattern
        .captures_iter(text)
        .map(|captures| {
            let degrees = required_number(captures.get(1).map(|group| group.as_str()), text)?;
            let minutes = optional_number(captures.get(2).map(|group| group.as_str()), text)?;
            let seconds = optional_number(captures.get(3).map(|group| group.as_str()), text)?;
            let hemisphere = captures
                .get(4)
                .and_then(|group| group.as_str().chars().next())
                .ok_or_else(|| Error::message(format!("Function `dms_point` could not read a hemisphere in `{text}`")))?
                .to_ascii_uppercase();

            Ok(Sexagesimal {
                // The reading is reduced to seconds and divided once, rather than summing
                // `degrees + minutes / 60 + seconds / 3600`. Neither sixtieth is representable in
                // binary, so the summed form rounds three times and lands one unit in the last place
                // above the correctly rounded result for about a quarter of all whole-second
                // readings. Here every term of a whole-second reading is an exact integer well
                // inside `f64`'s 53-bit range, so the sum is exact and the single division is the
                // only rounding: `86°55′30″` reads as exactly `86.925`.
                degrees: (degrees * 3600.0 + minutes * 60.0 + seconds) / 3600.0,
                hemisphere,
            })
        })
        .collect()
}

/// Reads a required numeric field, failing when it is absent or not a number.
fn required_number(captured: Option<&str>, text: &str) -> TeraResult<f64> {
    let Some(captured) = captured else {
        return Err(Error::message(format!("Function `dms_point` is missing a numeric field in `{text}`")));
    };

    captured
        .parse()
        .map_err(|_| Error::message(format!("Function `dms_point` could not read `{captured}` as a number in `{text}`")))
}

/// Reads an optional numeric field, defaulting to zero when the field is absent.
fn optional_number(captured: Option<&str>, text: &str) -> TeraResult<f64> {
    match captured {
        Some(captured) => required_number(Some(captured), text),
        None => Ok(0.0),
    }
}

/// Applies the hemisphere's sign: south and west are negative, north and east positive.
fn signed_degrees(reading: &Sexagesimal) -> f64 {
    if matches!(reading.hemisphere, 'S' | 'W') {
        -reading.degrees
    } else {
        reading.degrees
    }
}

/// Records a reading on its axis, rejecting a second reading for the same axis.
fn set_once(slot: &mut Option<f64>, value: f64, axis: &str, text: &str) -> TeraResult<()> {
    if slot.is_some() {
        return Err(Error::message(format!("Function `dms_point` found two {axis} readings in `{text}`")));
    }
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::template::function::dms_point::dms_point;
    use serde_json::json;
    use tera::{Context, Tera, Value};

    fn render(value: Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        tera.register_function("dms_point", dms_point);
        tera.add_raw_template("t", "{{ dms_point(value=value) }}").unwrap();

        let mut context = Context::new();
        context.insert_value("value", value);

        tera.render("t", &context)
    }

    fn point(value: &str) -> serde_json::Value {
        let rendered = render(Value::from(value)).unwrap();
        serde_json::from_str(&rendered).unwrap()
    }

    fn coordinates(value: &str) -> (f64, f64) {
        let point = point(value);
        let coordinates = point.get("coordinates").unwrap().as_array().unwrap();
        (coordinates[0].as_f64().unwrap(), coordinates[1].as_f64().unwrap())
    }

    #[test]
    fn a_labelled_pair_parses_into_a_geojson_point_in_longitude_latitude_order() {
        assert_eq!(point("27°59′17″N 86°55′30″E").get("type").unwrap(), &json!("Point"));

        let (longitude, latitude) = coordinates("27°59′17″N 86°55′30″E");
        assert!((longitude - 86.925).abs() < 1e-6);
        assert!((latitude - 27.988_055_6).abs() < 1e-6);
    }

    #[test]
    fn a_whole_second_reading_converts_to_its_exact_decimal_degrees() {
        // 86°55′30″ is 86 + 11/12 + 1/120 = 3477/40, which is exactly 86.925: a reading whose exact
        // value a float can hold must not come out a unit in the last place away from it.
        let (longitude, latitude) = coordinates("27°59′17″N 86°55′30″E");

        // The bit patterns are compared rather than the values: the point of the assertion is that
        // the reading lands on the same `f64` the literal does, which a tolerance would not catch.
        assert_eq!(longitude.to_bits(), 86.925_f64.to_bits());
        // 27°59′17″ is 27 + 3557/3600, a repeating decimal, so the expectation is the nearest `f64`
        // to it rather than a terminating literal.
        assert_eq!(latitude.to_bits(), 27.988_055_555_555_555_f64.to_bits());
    }

    #[test]
    fn southern_and_western_hemispheres_are_negative() {
        let (longitude, latitude) = coordinates("32°39′14″S 70°00′40″W");
        assert!(longitude < 0.0);
        assert!(latitude < 0.0);
    }

    #[test]
    fn the_axes_may_be_written_in_either_order() {
        assert_eq!(coordinates("86°55′30″E 27°59′17″N"), coordinates("27°59′17″N 86°55′30″E"));
    }

    #[test]
    fn a_trailing_footnote_marker_after_the_hemisphere_is_ignored() {
        let (longitude, latitude) = coordinates("27°42′12″N 88°08′51″E *");
        assert!((longitude - 88.147_5).abs() < 1e-6);
        assert!((latitude - 27.703_3).abs() < 1e-3);
    }

    #[test]
    fn ascii_prime_markers_are_accepted() {
        assert_eq!(coordinates("27°59'17\"N 86°55'30\"E"), coordinates("27°59′17″N 86°55′30″E"));
    }

    #[test]
    fn a_reading_without_seconds_is_accepted() {
        let (longitude, latitude) = coordinates("45°30′N 9°12′E");
        assert!((longitude - 9.2).abs() < 1e-6);
        assert!((latitude - 45.5).abs() < 1e-6);
    }

    #[test]
    fn a_missing_axis_is_an_error() {
        assert!(render(Value::from("27°59′17″N")).is_err());
    }

    #[test]
    fn a_non_string_value_is_an_error() {
        assert!(render(Value::from(42)).is_err());
    }

    #[test]
    fn a_missing_value_argument_is_an_error() {
        let mut tera = Tera::default();
        tera.register_function("dms_point", dms_point);
        tera.add_raw_template("t", "{{ dms_point() }}").unwrap();

        assert!(tera.render("t", &Context::new()).is_err());
    }
}

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use tera::{Error, Kwargs, State, TeraResult, Value};

const DEFAULT_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Tera filter `date_subtract_seconds`: shifts a timestamp backwards by `seconds` and formats the
/// result with `format`, defaulting to RFC 3339 in UTC.
///
/// Used where a source publishes an end-of-interval timestamp but the entity needs the start of
/// the interval it describes.
///
/// # Errors
/// Returns a [`tera::Error`] when the value or the `seconds` argument is not a readable timestamp
/// or whole-second count.
// `Kwargs` is taken by value because tera's blanket `Filter` impl is `Fn(Arg, Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_filter` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Filter trait requires Kwargs by value")]
pub fn date_subtract_seconds(value: &Value, kwargs: Kwargs, _state: &State) -> TeraResult<String> {
    let format = kwargs.get::<String>("format")?.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let seconds = seconds_argument(kwargs.get::<Value>("seconds")?.as_ref())?;
    let timestamp = parse_timestamp(value)? - Duration::seconds(seconds);

    Ok(timestamp.format(&format).to_string())
}

/// Reads the `seconds` argument, which a mapping may write either as a number or as a string.
fn seconds_argument(argument: Option<&Value>) -> TeraResult<i64> {
    let Some(argument) = argument else {
        return Ok(0);
    };

    if let Some(number) = argument.as_number() {
        return number
            .is_integer()
            .then(|| number.as_integer().and_then(|seconds| i64::try_from(seconds).ok()))
            .flatten()
            .ok_or_else(|| Error::message(format!("Filter `date_subtract_seconds` needs whole seconds, got {number}")));
    }

    if let Some(text) = argument.as_str() {
        return text
            .parse()
            .map_err(|_| Error::message(format!("Filter `date_subtract_seconds` could not read `{text}` as whole seconds")));
    }

    Err(Error::message(format!(
        "Filter `date_subtract_seconds` needs a number or string for `seconds`, got {argument}"
    )))
}

/// Reads a timestamp from either a Unix epoch number or one of the textual shapes real sources
/// emit: RFC 3339, a space-separated offset form, a naive datetime, or a bare date.
fn parse_timestamp(value: &Value) -> TeraResult<DateTime<Utc>> {
    if let Some(epoch) = value.as_i64() {
        return DateTime::from_timestamp(epoch, 0).ok_or_else(|| Error::message(format!("Epoch second {epoch} is out of range")));
    }

    if let Some(text) = value.as_str() {
        return parse_textual_timestamp(text);
    }

    Err(Error::message(format!(
        "Filter `date_subtract_seconds` needs a number or string timestamp, got {value}"
    )))
}

/// Tries each accepted textual timestamp shape in turn, most specific first.
fn parse_textual_timestamp(text: &str) -> TeraResult<DateTime<Utc>> {
    text.parse::<DateTime<FixedOffset>>()
        .map(|parsed| parsed.with_timezone(&Utc))
        .or_else(|_| DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%:z").map(|parsed| parsed.with_timezone(&Utc)))
        .or_else(|_| {
            text.parse::<NaiveDateTime>()
                .map(|parsed| DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc))
        })
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").map(|parsed| DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc)))
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|parsed| DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc))
        })
        .ok_or_else(|| Error::message(format!("Error parsing `{text}` as datetime")))
}

#[cfg(test)]
mod tests {
    use crate::template::filter::timestamp::{date_subtract_seconds, parse_textual_timestamp, seconds_argument};
    use chrono::{DateTime, Utc};
    use tera::{Context, Tera, Value};

    fn render(value: Value, seconds: Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        tera.register_filter("date_subtract_seconds", date_subtract_seconds);
        tera.add_raw_template("t", "{{ value | date_subtract_seconds(seconds=seconds) }}").unwrap();

        let mut context = Context::new();
        context.insert_value("value", value);
        context.insert_value("seconds", seconds);

        tera.render("t", &context)
    }

    #[test]
    fn subtracts_from_an_rfc_3339_timestamp() {
        assert_eq!(render(Value::from("2026-04-03T22:00:20Z"), Value::from(20)).unwrap(), "2026-04-03T22:00:00Z");
    }

    #[test]
    fn subtracts_from_an_epoch_second() {
        assert_eq!(render(Value::from(1_775_253_620_i64), Value::from(20)).unwrap(), "2026-04-03T22:00:00Z");
    }

    #[test]
    fn accepts_a_space_separated_offset_timestamp() {
        assert_eq!(
            render(Value::from("2026-04-03 22:00:20+00:00"), Value::from(20)).unwrap(),
            "2026-04-03T22:00:00Z"
        );
    }

    #[test]
    fn accepts_a_naive_datetime() {
        assert_eq!(render(Value::from("2026-04-03T22:00:20"), Value::from(20)).unwrap(), "2026-04-03T22:00:00Z");
    }

    #[test]
    fn accepts_a_bare_date() {
        assert_eq!(render(Value::from("2026-04-03"), Value::from(0)).unwrap(), "2026-04-03T00:00:00Z");
    }

    #[test]
    fn reads_the_seconds_argument_written_as_a_string() {
        assert_eq!(render(Value::from("2026-04-03T22:00:20Z"), Value::from("20")).unwrap(), "2026-04-03T22:00:00Z");
    }

    #[test]
    fn honours_an_explicit_format_argument() {
        let mut tera = Tera::default();
        tera.register_filter("date_subtract_seconds", date_subtract_seconds);
        tera.add_raw_template("t", r#"{{ value | date_subtract_seconds(seconds=20, format="%Y-%m-%d %H:%M") }}"#)
            .unwrap();

        let mut context = Context::new();
        context.insert("value", "2026-04-03T22:00:20Z");

        assert_eq!(tera.render("t", &context).unwrap(), "2026-04-03 22:00");
    }

    #[test]
    fn defaults_to_no_shift_when_seconds_is_absent() {
        let mut tera = Tera::default();
        tera.register_filter("date_subtract_seconds", date_subtract_seconds);
        tera.add_raw_template("t", "{{ value | date_subtract_seconds }}").unwrap();

        let mut context = Context::new();
        context.insert("value", "2026-04-03T22:00:20Z");

        assert_eq!(tera.render("t", &context).unwrap(), "2026-04-03T22:00:20Z");
    }

    #[test]
    fn an_unparseable_timestamp_is_an_error() {
        assert!(render(Value::from("not a timestamp"), Value::from(0)).is_err());
    }

    #[test]
    fn a_fractional_seconds_argument_is_an_error() {
        assert!(render(Value::from("2026-04-03T22:00:20Z"), Value::from(1.5)).is_err());
    }

    #[test]
    fn the_seconds_argument_defaults_to_zero_when_absent() {
        assert_eq!(seconds_argument(None).unwrap(), 0);
    }

    #[test]
    fn a_string_seconds_argument_is_parsed() {
        assert_eq!(seconds_argument(Some(&Value::from("45"))).unwrap(), 45);
    }

    #[test]
    fn a_textual_bare_date_parses_to_midnight_utc() {
        let expected = "2026-04-03T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        assert_eq!(parse_textual_timestamp("2026-04-03").unwrap(), expected);
    }
}

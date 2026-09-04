use serde_json::Value as JsonValue;
use tera::{Error, Kwargs, State, Tera, TeraResult, Value};

/// Registers the `json_decode` filter on a Tera engine.
///
/// A text-shaped source (a CSV cell, an XML text node) often carries a whole JSON document as a
/// string: a movie's cast list, a device's raw payload. `json_decode` parses that string into a
/// structured value so the rest of the template can index and iterate it: `{{ cast | json_decode |
/// first | get(key="id") }}`, or `{% for member in cast | json_decode %}...{% endfor %}`. It is the
/// decode counterpart of `tera-contrib`'s `json_encode`.
///
/// A value that is not a string is passed through unchanged, so the filter is idempotent on a source
/// that already arrived structured; an empty or absent value yields a null, so the attribute simply
/// drops. Only a non-empty string that is not valid JSON is an error, since that is malformed source
/// data worth surfacing rather than silently discarding.
pub fn register(tera: &mut Tera) {
    tera.register_filter("json_decode", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        decode(value)
    });
}

/// Parses a JSON string into a Tera value, passing a non-string through and mapping an empty value to
/// a null.
fn decode(value: &Value) -> TeraResult<Value> {
    let Some(text) = value.as_str() else {
        return Ok(value.clone());
    };
    if text.trim().is_empty() {
        return Ok(Value::none());
    }

    let parsed: JsonValue = serde_json::from_str(text).map_err(|error| Error::message(format!("`json_decode` value {text:?} is not valid JSON: {error}")))?;
    Value::try_from_serializable(&parsed)
}

#[cfg(test)]
mod tests {
    use crate::template::filter::json_decode::register;
    use tera::{Context, Tera, Value};

    fn render(template: &str, value: Value) -> Result<String, tera::Error> {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        context.insert_value("value", value);

        tera.render("t", &context)
    }

    #[test]
    fn a_json_array_string_is_parsed_and_indexed() {
        let cast = Value::from(r#"[{"id": 31, "character": "Jack Sparrow"}, {"id": 32}]"#);
        assert_eq!(render(r#"{{ value | json_decode | first | get(key="id") }}"#, cast).unwrap(), "31");
    }

    #[test]
    fn a_json_array_string_is_iterable() {
        let cast = Value::from(r#"[{"id": 10}, {"id": 20}, {"id": 30}]"#);
        assert_eq!(
            render(r"{% for member in value | json_decode %}{{ member.id }} {% endfor %}", cast).unwrap(),
            "10 20 30 "
        );
    }

    #[test]
    fn the_decoded_length_is_available() {
        let cast = Value::from(r#"[{"id": 1}, {"id": 2}]"#);
        assert_eq!(render("{{ value | json_decode | length }}", cast).unwrap(), "2");
    }

    #[test]
    fn an_empty_value_decodes_to_nothing() {
        assert_eq!(render("{{ value | json_decode }}", Value::from("")).unwrap(), "");
    }

    #[test]
    fn a_non_string_value_passes_through_unchanged() {
        assert_eq!(render("{{ value | json_decode }}", Value::from(42)).unwrap(), "42");
    }

    #[test]
    fn a_malformed_json_string_is_an_error() {
        assert!(render("{{ value | json_decode }}", Value::from("{not json")).is_err());
    }
}

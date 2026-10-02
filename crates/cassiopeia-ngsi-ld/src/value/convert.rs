use crate::value::types::{Number, TemporalValue, Value, ValueObject};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde_json::Value as JsonValue;

impl Value {
    #[must_use]
    pub fn into_json_value(self) -> JsonValue {
        JsonValue::from(self)
    }

    /// Parses the value into an instant, returning `None` when it carries no parseable one.
    ///
    /// Text is read in any supported spelling and a number as a Unix epoch (see
    /// [`TemporalValue::from_number`]); no other shape holds an instant.
    #[must_use]
    pub fn try_parse_datetime(&self) -> Option<DateTime<Utc>> {
        let parsed = match self {
            Value::Temporal(TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt)) => return Some(*dt),
            Value::String(s) => TemporalValue::try_parse(s),
            Value::Number(number) => TemporalValue::from_number(number),
            Value::Null | Value::Boolean(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        };
        parsed.map(|t| match t {
            TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => dt,
        })
    }
}

/// Parses an instant straight out of a JSON value, without building a [`Value`] first.
///
/// `JsonValue` carries no temporal variant, so only a string or a number can hold an instant: every
/// other variant would allocate a `Value` only for [`Value::try_parse_datetime`] to reject it. The
/// result is identical to converting first.
#[must_use]
pub fn parse_datetime(value: &JsonValue) -> Option<DateTime<Utc>> {
    let parsed = match value {
        JsonValue::String(text) => TemporalValue::try_parse(text),
        JsonValue::Number(number) => json_number(number).and_then(|number| TemporalValue::from_number(&number)),
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Array(_) | JsonValue::Object(_) => None,
    };
    parsed.map(|t| match t {
        TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => dt,
    })
}

/// Reads a JSON number as a [`Number`]: an integer when it fits `i64`, otherwise a float.
fn json_number(number: &serde_json::Number) -> Option<Number> {
    number.as_i64().map(Number::Integer).or_else(|| number.as_f64().map(Number::Float))
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Number(Number::Integer(i))
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Number(Number::Float(f))
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl From<JsonValue> for Value {
    fn from(json: JsonValue) -> Self {
        match json {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Boolean(b),
            JsonValue::Number(n) => json_number(&n).map_or(Value::Null, Value::Number),
            JsonValue::String(s) => Value::String(CompactString::from(s)),
            JsonValue::Array(arr) => Value::Array(arr.into_iter().map(Value::from).collect()),
            JsonValue::Object(obj) => {
                let mut map = ValueObject::default();
                for (k, v) in obj {
                    map.insert(CompactString::from(k), Value::from(v));
                }
                Value::Object(Box::new(map))
            }
        }
    }
}

impl From<Value> for JsonValue {
    fn from(val: Value) -> Self {
        match val {
            Value::Null => JsonValue::Null,
            Value::Boolean(b) => JsonValue::Bool(b),
            Value::Number(n) => match n {
                Number::Integer(i) => JsonValue::Number(i.into()),
                Number::Float(f) => serde_json::Number::from_f64(f).map_or(JsonValue::Null, JsonValue::Number),
            },
            Value::String(s) => JsonValue::String(s.to_string()),
            Value::Temporal(t) => match t {
                TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => JsonValue::String(dt.to_rfc3339()),
            },
            Value::Geospatial(g) => serde_json::to_value(g).unwrap_or(JsonValue::Null),
            Value::Array(arr) => JsonValue::Array(arr.into_iter().map(JsonValue::from).collect()),
            Value::Object(obj) => {
                let mut map = serde_json::Map::new();
                for (k, v) in *obj {
                    map.insert(k.to_string(), JsonValue::from(v));
                }
                JsonValue::Object(map)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::{
        convert::parse_datetime,
        types::{Number, Value},
    };
    use serde_json::json;

    #[test]
    fn parse_datetime_matches_converting_the_json_value_first() {
        for value in [
            json!("2026-04-03T22:00:20Z"),
            json!("2026-04-03"),
            json!("not a timestamp"),
            json!(1_744_000_000),
            json!(1_744_000_000_250_i64),
            json!(1_744_000_000.5),
            json!(2026),
            json!(true),
            json!(null),
            json!(["2026-04-03T22:00:20Z"]),
            json!({"at": "2026-04-03T22:00:20Z"}),
        ] {
            assert_eq!(parse_datetime(&value), Value::from(value.clone()).try_parse_datetime(), "mismatch for {value}");
        }
    }

    #[test]
    fn parse_datetime_reads_an_rfc_3339_string_as_an_instant() {
        assert!(parse_datetime(&json!("2026-04-03T22:00:20Z")).is_some());
        assert!(parse_datetime(&json!("2026-04-03")).is_some());
        assert!(parse_datetime(&json!("not a timestamp")).is_none());
    }

    #[test]
    fn parse_datetime_reads_an_epoch_number_as_the_same_instant_as_its_text() {
        assert!(parse_datetime(&json!(1_775_253_620)).is_some());
        assert_eq!(parse_datetime(&json!(1_775_253_620)), parse_datetime(&json!("1775253620")));
        assert_eq!(parse_datetime(&json!(1_775_253_620.5)), parse_datetime(&json!("1775253620.5")));
        assert!(parse_datetime(&json!(2026)).is_none());
    }

    #[test]
    fn an_epoch_number_value_parses_to_an_instant() {
        assert!(Value::from(1_775_253_620_i64).try_parse_datetime().is_some());
        assert!(Value::from(1_775_253_620.5).try_parse_datetime().is_some());
        assert!(Value::from(2026_i64).try_parse_datetime().is_none());
    }

    #[test]
    fn json_numbers_round_trip_as_integers_or_floats() {
        assert_eq!(Value::from(json!(42)), Value::Number(Number::Integer(42)));
        assert_eq!(Value::from(json!(1.5)), Value::Number(Number::Float(1.5)));
    }

    #[test]
    fn a_json_array_round_trips_through_value() {
        let value = Value::from(json!([1, "two", true]));
        assert_eq!(serde_json::Value::from(value), json!([1, "two", true]));
    }
}

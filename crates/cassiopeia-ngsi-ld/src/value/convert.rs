use crate::value::{
    finite_float::FiniteFloat,
    non_finite_float::NonFiniteFloat,
    null_in_json::NullInJson,
    omitted_nulls::OmittedNulls,
    types::{Number, TemporalValue, Value, ValueObject},
};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use geojson::GeometryValue;
use serde_json::Value as JsonValue;

impl Value {
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
            Value::Boolean(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        };
        parsed.map(|t| match t {
            TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => dt,
        })
    }
}

/// Reads a JSON number as a [`Number`]: an integer when it fits `i64`, otherwise a float.
///
/// A JSON number is always finite (RFC 8259 clause 6), so the float check refuses nothing here.
fn json_number(number: &serde_json::Number) -> Option<Number> {
    number
        .as_i64()
        .map(Number::Integer)
        .or_else(|| number.as_f64().and_then(|float| FiniteFloat::try_from(float).ok()).map(Number::Float))
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Number(Number::Integer(i))
    }
}

impl From<FiniteFloat> for Value {
    fn from(f: FiniteFloat) -> Self {
        Value::Number(Number::Float(f))
    }
}

/// Reads a float as a number value, refusing `NaN` and the infinities, for which JSON has no number
/// (RFC 8259 clause 6).
impl TryFrom<f64> for Value {
    type Error = NonFiniteFloat;

    fn try_from(f: f64) -> Result<Value, NonFiniteFloat> {
        FiniteFloat::try_from(f).map(Value::from)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl Value {
    /// Reads a JSON document as an NGSI-LD value, leaving out every null it holds.
    ///
    /// `null` is no NGSI-LD Value (ETSI GS CIM 009 v1.9.1 clause 3.1), and none of an entity's
    /// members admits one, at any depth: JSON-LD expansion removes a null entry or list item without
    /// a word (clauses 4.5.0 and 5.2.1), so a broker would store something other than what was
    /// written. Leaving the null out here keeps the same members and items a broker would keep, and
    /// counts each one left out in `omitted`, so the loss is reported where it happens instead of
    /// vanishing on the broker.
    ///
    /// A top-level null is no value at all and yields `None`. It is not counted: a field the record
    /// does not carry is an absent attribute, not a loss inside one.
    pub fn from_json(json: JsonValue, omitted: &mut OmittedNulls) -> Option<Value> {
        match json {
            JsonValue::Null => None,
            JsonValue::Bool(b) => Some(Value::Boolean(b)),
            JsonValue::Number(n) => json_number(&n).map(Value::Number),
            JsonValue::String(s) => Some(Value::String(CompactString::from(s))),
            JsonValue::Array(items) => Some(Value::Array(
                items
                    .into_iter()
                    .filter_map(|item| {
                        if item.is_null() {
                            omitted.array_items += 1;
                            return None;
                        }
                        Value::from_json(item, omitted)
                    })
                    .collect(),
            )),
            JsonValue::Object(members) => {
                let mut object = ValueObject::default();
                for (key, member) in members {
                    if member.is_null() {
                        omitted.object_members += 1;
                        continue;
                    }
                    if let Some(value) = Value::from_json(member, omitted) {
                        object.insert(CompactString::from(key), value);
                    }
                }
                Some(Value::Object(Box::new(object)))
            }
        }
    }
}

/// Reads a JSON document that holds no null anywhere as an NGSI-LD value.
///
/// The strict counterpart of [`Value::from_json`], for a caller that holds JSON it expects to be
/// a valid NGSI-LD Value as it stands, so that a null in it is a mistake to surface rather than a
/// member to leave out.
impl TryFrom<JsonValue> for Value {
    type Error = NullInJson;

    fn try_from(json: JsonValue) -> Result<Value, NullInJson> {
        let mut omitted = OmittedNulls::default();
        let value = Value::from_json(json, &mut omitted).ok_or(NullInJson)?;
        if omitted.is_empty() { Ok(value) } else { Err(NullInJson) }
    }
}

/// Writes a value as the raw JSON it serializes to, built without an intermediate encoding.
///
/// Every value has a JSON form: a float is always finite, so it is always a JSON number (RFC 8259
/// clause 6), a time value is its clause 4.6.3 text, and a geometry is its `GeoJSON` object (RFC
/// 7946 clause 3.1). The JSON holds no null, because a value never does (ETSI GS CIM 009 v1.9.1
/// clause 4.5.0).
// Manual because each variant maps to a distinct JSON shape, as the value's `Serialize` does, which
// no derive expresses.
impl From<Value> for JsonValue {
    fn from(value: Value) -> JsonValue {
        match value {
            Value::Boolean(b) => JsonValue::Bool(b),
            Value::Number(Number::Integer(i)) => JsonValue::Number(i.into()),
            Value::Number(Number::Float(f)) => JsonValue::from(f.get()),
            Value::String(text) => JsonValue::String(text.into()),
            Value::Temporal(temporal) => JsonValue::String(temporal.text().to_string()),
            Value::Geospatial(geometry) => JsonValue::from(&GeometryValue::from(geometry.as_ref())),
            Value::Array(items) => JsonValue::Array(items.into_iter().map(JsonValue::from).collect()),
            Value::Object(members) => JsonValue::Object(members.into_iter().map(|(key, member)| (key.into(), JsonValue::from(member))).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::{
        finite_float::FiniteFloat,
        non_finite_float::NonFiniteFloat,
        null_in_json::NullInJson,
        omitted_nulls::OmittedNulls,
        types::{Number, TemporalValue, Value},
    };
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use chrono::{TimeZone, Utc};
    use compact_str::CompactString;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn read(json: serde_json::Value) -> (Option<Value>, OmittedNulls) {
        let mut omitted = OmittedNulls::default();
        let value = Value::from_json(json, &mut omitted);
        (value, omitted)
    }

    #[test]
    fn an_rfc_3339_string_value_parses_to_an_instant() {
        assert!(Value::try_from(json!("2026-04-03T22:00:20Z")).unwrap().try_parse_datetime().is_some());
        assert!(Value::try_from(json!("2026-04-03")).unwrap().try_parse_datetime().is_some());
        assert!(Value::try_from(json!("not a timestamp")).unwrap().try_parse_datetime().is_none());
    }

    #[test]
    fn an_epoch_number_value_parses_to_the_same_instant_as_its_text() {
        assert_eq!(
            Value::try_from(json!(1_775_253_620)).unwrap().try_parse_datetime(),
            Value::try_from(json!("1775253620")).unwrap().try_parse_datetime()
        );
        assert_eq!(
            Value::try_from(json!(1_775_253_620.5)).unwrap().try_parse_datetime(),
            Value::try_from(json!("1775253620.5")).unwrap().try_parse_datetime()
        );
    }

    #[test]
    fn a_value_shaped_as_no_instant_parses_to_none() {
        for value in [json!(true), json!(["2026-04-03T22:00:20Z"]), json!({"at": "2026-04-03T22:00:20Z"})] {
            assert!(Value::try_from(value.clone()).unwrap().try_parse_datetime().is_none(), "{value}");
        }
    }

    #[test]
    fn an_epoch_number_value_parses_to_an_instant() {
        assert!(Value::from(1_775_253_620_i64).try_parse_datetime().is_some());
        assert!(Value::try_from(1_775_253_620.5).unwrap().try_parse_datetime().is_some());
        assert_eq!(Value::from(2026_i64).try_parse_datetime(), None);
    }

    #[test]
    fn json_numbers_read_as_integers_or_floats() {
        assert_eq!(Value::try_from(json!(42)).unwrap(), Value::Number(Number::Integer(42)));
        assert_eq!(
            Value::try_from(json!(1.5)).unwrap(),
            Value::Number(Number::Float(FiniteFloat::try_from(1.5).unwrap()))
        );
    }

    #[test]
    fn a_finite_float_becomes_a_number_value() {
        assert_eq!(Value::try_from(1.5), Ok(Value::Number(Number::Float(FiniteFloat::try_from(1.5).unwrap()))));
    }

    #[test]
    fn a_float_that_is_not_finite_is_no_value() {
        assert_eq!(Value::try_from(f64::INFINITY), Err(NonFiniteFloat(f64::INFINITY)));
        assert!(Value::try_from(f64::NAN).is_err());
    }

    #[test]
    fn a_top_level_null_is_no_value_and_is_not_counted_as_omitted() {
        let (value, omitted) = read(json!(null));

        assert_eq!(value, None);
        assert_eq!(omitted, OmittedNulls::default());
    }

    #[test]
    fn a_null_array_item_is_left_out_and_counted() {
        let (value, omitted) = read(json!(["a", null, "b"]));

        assert_eq!(value, Some(Value::try_from(json!(["a", "b"])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn a_null_object_member_is_left_out_and_counted() {
        let (value, omitted) = read(json!({"value": 21.5, "unit": null}));

        assert_eq!(value, Some(Value::try_from(json!({"value": 21.5})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
        );
    }

    #[test]
    fn nulls_nested_at_any_depth_are_left_out_and_counted_together() {
        let (value, omitted) = read(json!({"a": [null, {"b": null, "c": [1, null]}]}));

        assert_eq!(value, Some(Value::try_from(json!({"a": [{"c": [1]}]})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 2,
                object_members: 1,
            }
        );
    }

    #[test]
    fn an_empty_string_is_kept_as_a_value() {
        let (value, omitted) = read(json!(""));

        assert_eq!(value, Some(Value::String(CompactString::new(""))));
        assert!(omitted.is_empty());
    }

    #[test]
    fn the_strict_conversion_refuses_json_holding_a_null_anywhere() {
        assert_eq!(Value::try_from(json!(null)), Err(NullInJson));
        assert_eq!(Value::try_from(json!(["a", null])), Err(NullInJson));
        assert_eq!(Value::try_from(json!({"value": 21.5, "unit": null})), Err(NullInJson));
    }

    #[test]
    fn the_strict_conversion_accepts_json_holding_no_null() {
        assert_eq!(
            Value::try_from(json!([1, "two", true])).unwrap(),
            Value::Array(vec![Value::from(1_i64), Value::String(CompactString::from("two")), Value::Boolean(true)])
        );
    }

    #[test]
    fn a_value_converts_into_the_raw_json_it_serializes_to() {
        let instant = Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 35).unwrap();
        let point = Value::Geospatial(Box::new(NgsiLdGeometry::Point {
            coordinates: [16.37, 48.2].into(),
        }));
        let values = [
            Value::Number(Number::Float(FiniteFloat::try_from(2.5).unwrap())),
            Value::Temporal(TemporalValue::DateTime(instant)),
            point,
            Value::try_from(json!({"a": [1, "two", true], "b": {"c": 3.0}})).unwrap(),
        ];

        for value in values {
            let expected = serde_json::to_value(&value).unwrap();

            assert_eq!(JsonValue::from(value), expected);
        }
        assert_eq!(
            JsonValue::from(Value::Temporal(TemporalValue::DateTime(instant))),
            json!("2026-03-01T11:04:35Z")
        );
        assert_eq!(
            JsonValue::from(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [16.37, 48.2].into(),
            }))),
            json!({"type": "Point", "coordinates": [16.37, 48.2]})
        );
    }
}

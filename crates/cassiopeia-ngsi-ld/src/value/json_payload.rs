use crate::value::{
    json_payload_error::{JsonPayloadError, JsonShape},
    types::{Number, Value},
};
use geojson::GeometryValue;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};

/// One JSON object, as raw JSON that NGSI-LD never interprets.
pub type JsonObject = Map<String, JsonValue>;

/// The `json` member of a `JsonProperty`: "a raw JSON object (or array of objects)" (ETSI GS CIM 009
/// v1.9.1 clause 4.5.24.2).
///
/// The core `@context` types `json` as `@json`, so its contents "are never subject to JSON-LD term
/// expansion or compaction" (clause 4.5.24.2). That is why a null nested inside it is kept exactly as
/// the source has it: JSON-LD expansion removes no member of a JSON literal (clauses 4.5.0 and
/// 5.2.1). The member as a whole is never null and never a scalar, because neither is a JSON
/// object, so neither can be built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonPayload {
    /// A single JSON object.
    Object(JsonObject),
    /// An array of JSON objects.
    Objects(Vec<JsonObject>),
}

/// Takes a JSON value that is an object, or names the shape it has instead.
fn into_object(json: JsonValue) -> Result<JsonObject, JsonShape> {
    match json {
        JsonValue::Object(object) => Ok(object),
        JsonValue::Null => Err(JsonShape::Null),
        JsonValue::Bool(_) => Err(JsonShape::Boolean),
        JsonValue::Number(_) => Err(JsonShape::Number),
        JsonValue::String(_) => Err(JsonShape::String),
        JsonValue::Array(_) => Err(JsonShape::Array),
    }
}

impl TryFrom<JsonValue> for JsonPayload {
    type Error = JsonPayloadError;

    fn try_from(json: JsonValue) -> Result<JsonPayload, JsonPayloadError> {
        match json {
            JsonValue::Array(items) => items
                .into_iter()
                .map(|item| into_object(item).map_err(|found| JsonPayloadError::ItemNotAnObject { found }))
                .collect::<Result<Vec<JsonObject>, JsonPayloadError>>()
                .map(JsonPayload::Objects),
            other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Object(_)) => into_object(other)
                .map(JsonPayload::Object)
                .map_err(|found| JsonPayloadError::NotAnObject { found }),
        }
    }
}

// Manual because the payload unwraps into the JSON shape of its variant, which no derive expresses.
impl From<JsonPayload> for JsonValue {
    fn from(payload: JsonPayload) -> JsonValue {
        match payload {
            JsonPayload::Object(object) => JsonValue::Object(object),
            JsonPayload::Objects(objects) => JsonValue::Array(objects.into_iter().map(JsonValue::Object).collect()),
        }
    }
}

/// Builds a payload from a value a declared transformation produced, as the raw JSON it serializes
/// to.
///
/// A `JsonProperty` that names a transformation (such as `object`) converts its source the way any
/// attribute does, and the result is then carried as raw JSON. It holds no null, because a
/// [`Value`] never does.
impl TryFrom<Value> for JsonPayload {
    type Error = JsonPayloadError;

    fn try_from(value: Value) -> Result<JsonPayload, JsonPayloadError> {
        JsonPayload::try_from(raw_json(value))
    }
}

/// The raw JSON a value serializes to, built without an intermediate encoding.
///
/// Every value has a JSON form: a float is always finite, so it is always a JSON number (RFC 8259
/// clause 6).
fn raw_json(value: Value) -> JsonValue {
    match value {
        Value::Boolean(b) => JsonValue::Bool(b),
        Value::Number(Number::Integer(i)) => JsonValue::Number(i.into()),
        Value::Number(Number::Float(f)) => JsonValue::from(f.get()),
        Value::String(text) => JsonValue::String(text.into()),
        Value::Temporal(temporal) => JsonValue::String(temporal.text().to_string()),
        Value::Geospatial(geometry) => JsonValue::from(&GeometryValue::from(geometry.as_ref())),
        Value::Array(items) => JsonValue::Array(items.into_iter().map(raw_json).collect()),
        Value::Object(members) => JsonValue::Object(members.into_iter().map(|(key, member)| (key.into(), raw_json(member))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use crate::value::{
        json_payload::JsonPayload,
        json_payload_error::{JsonPayloadError, JsonShape},
        types::Value,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn an_object_is_a_payload_and_keeps_a_nested_null() {
        let payload = JsonPayload::try_from(json!({"maybe": null})).unwrap();

        assert_eq!(serde_json::to_value(&payload).unwrap(), json!({"maybe": null}));
    }

    #[test]
    fn an_array_of_objects_is_a_payload() {
        let payload = JsonPayload::try_from(json!([{"a": 1}, {"b": null}])).unwrap();

        assert_eq!(serde_json::to_value(&payload).unwrap(), json!([{"a": 1}, {"b": null}]));
    }

    #[test]
    fn an_empty_array_is_a_payload_of_no_objects() {
        assert_eq!(JsonPayload::try_from(json!([])).unwrap(), JsonPayload::Objects(Vec::new()));
    }

    #[test]
    fn a_null_or_a_scalar_is_refused_naming_its_shape() {
        for (json, found) in [
            (json!(null), JsonShape::Null),
            (json!(true), JsonShape::Boolean),
            (json!(3.5), JsonShape::Number),
            (json!("text"), JsonShape::String),
        ] {
            assert_eq!(JsonPayload::try_from(json), Err(JsonPayloadError::NotAnObject { found }));
        }
    }

    #[test]
    fn an_array_holding_anything_but_objects_is_refused_naming_the_item_s_shape() {
        assert_eq!(
            JsonPayload::try_from(json!([{"a": 1}, 2])),
            Err(JsonPayloadError::ItemNotAnObject { found: JsonShape::Number })
        );
        assert_eq!(
            JsonPayload::try_from(json!([[{"a": 1}]])),
            Err(JsonPayloadError::ItemNotAnObject { found: JsonShape::Array })
        );
        assert_eq!(
            JsonPayload::try_from(json!([null])),
            Err(JsonPayloadError::ItemNotAnObject { found: JsonShape::Null })
        );
    }

    #[test]
    fn a_transformed_object_value_becomes_the_payload_it_serializes_to() {
        let value = Value::try_from(json!({"value": 21.5, "tags": ["a", 1, true]})).unwrap();

        assert_eq!(
            serde_json::to_value(JsonPayload::try_from(value).unwrap()).unwrap(),
            json!({"value": 21.5, "tags": ["a", 1, true]})
        );
    }

    #[test]
    fn a_transformed_scalar_value_is_refused() {
        assert_eq!(
            JsonPayload::try_from(Value::Boolean(true)),
            Err(JsonPayloadError::NotAnObject { found: JsonShape::Boolean })
        );
    }

    #[test]
    fn a_payload_unwraps_into_the_json_it_holds() {
        assert_eq!(JsonValue::from(JsonPayload::try_from(json!([{"a": null}])).unwrap()), json!([{"a": null}]));
        assert_eq!(JsonValue::from(JsonPayload::try_from(json!({"a": 1})).unwrap()), json!({"a": 1}));
    }

    #[test]
    fn a_payload_round_trips_through_json() {
        let payload = JsonPayload::try_from(json!([{"a": [1, null]}])).unwrap();
        let decoded: JsonPayload = serde_json::from_str(&serde_json::to_string(&payload).unwrap()).unwrap();

        assert_eq!(decoded, payload);
    }
}

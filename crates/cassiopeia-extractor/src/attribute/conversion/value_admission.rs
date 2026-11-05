use crate::{attribute::refusal::AttributeRefusal, required_value::RequiredValue};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// Admits raw JSON as the NGSI-LD Value a `Property` or a `ListProperty` holds.
///
/// No null can sit anywhere in a Value (ETSI GS CIM 009 v1.9.1 clause 4.5.0), so every null nested
/// in the JSON is left out and counted in `omitted`. An empty object or array is a value the record
/// does not carry and admits nothing, as an absent source does. An object or array that held
/// something but nothing except nulls is refused instead: the source carried it, and writing it
/// without its nulls would leave nothing, so the run names it rather than losing it unannounced.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting an object or array of only nulls as not
/// [`RequiredValue::NonNull`].
pub(crate) fn admit(json: JsonValue, omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
    if holds_only_nulls(&json) {
        return Err(AttributeRefusal::unusable(RequiredValue::NonNull, &json.to_string()));
    }

    Ok(Value::from_json(json, omitted).filter(|value| !is_empty_container(value)))
}

/// Whether the JSON is an object or array holding at least one member or item, every one null.
fn holds_only_nulls(json: &JsonValue) -> bool {
    match json {
        JsonValue::Array(items) => !items.is_empty() && items.iter().all(JsonValue::is_null),
        JsonValue::Object(members) => !members.is_empty() && members.values().all(JsonValue::is_null),
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => false,
    }
}

/// Whether the value is an object or array with nothing in it.
fn is_empty_container(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(members) => members.is_empty(),
        Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{conversion::value_admission::admit, refusal::AttributeRefusal},
        required_value::RequiredValue,
    };
    use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn admitted(json: JsonValue) -> (Result<Option<Value>, AttributeRefusal>, OmittedNulls) {
        let mut omitted = OmittedNulls::default();
        let admitted = admit(json, &mut omitted);
        (admitted, omitted)
    }

    #[test]
    fn json_is_admitted_without_its_nulls_and_counts_them() {
        let (value, omitted) = admitted(json!({"value": 21.5, "unit": null, "tags": ["a", null]}));

        assert_eq!(value, Ok(Some(Value::try_from(json!({"value": 21.5, "tags": ["a"]})).unwrap())));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 1,
            }
        );
    }

    #[test]
    fn an_object_or_array_of_only_nulls_is_refused_quoting_it_and_counts_nothing() {
        for (json, quoted) in [(json!({"maybe": null}), r#"{"maybe":null}"#), (json!([null, null]), "[null,null]")] {
            let (value, omitted) = admitted(json);

            assert_eq!(value, Err(AttributeRefusal::unusable(RequiredValue::NonNull, quoted)));
            assert!(omitted.is_empty(), "{quoted}");
        }
    }

    #[test]
    fn an_empty_object_or_array_is_absent() {
        for json in [json!({}), json!([])] {
            assert_eq!(admitted(json).0, Ok(None));
        }
    }

    #[test]
    fn an_object_holding_an_object_of_only_nulls_keeps_the_emptied_member() {
        let (value, omitted) = admitted(json!({"a": {"b": null}}));

        assert_eq!(value, Ok(Some(Value::try_from(json!({"a": {}})).unwrap())));
        assert_eq!(omitted.object_members, 1);
    }
}

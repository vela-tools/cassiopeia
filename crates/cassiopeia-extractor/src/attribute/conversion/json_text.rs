use crate::{
    attribute::{
        conversion::source_parts::{SourceParts, is_absent},
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_ngsi_ld::value::json_payload::JsonObject;
use serde_json::Value as JsonValue;

/// Reads text the record carries as the JSON it is written as (RFC 8259 clause 2).
///
/// Text spelling `null` reads as the null it spells; a reading that needs something else refuses it
/// as it refuses any other JSON of the wrong shape.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting the text, as not the `required` value, when
/// it is not JSON.
fn parse_json(text: &str, required: RequiredValue) -> Result<JsonValue, AttributeRefusal> {
    serde_json::from_str(text).map_err(|_| AttributeRefusal::unusable(required, text))
}

/// Reads a value the record carries as the JSON it stands for: text as the JSON value it is written
/// as, which may be the null `null` spells, and any other value as it is, every nested null kept.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting text that is not JSON as not
/// [`RequiredValue::Json`].
pub(crate) fn json_of(value: JsonValue) -> Result<JsonValue, AttributeRefusal> {
    match value {
        JsonValue::String(text) => parse_json(&text, RequiredValue::Json),
        other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => Ok(other),
    }
}

/// Reads every part as the JSON it stands for (see [`json_of`]), turning a part that is absent (see
/// [`is_absent`]) into a null, so the parts keep their positions and an absent one is left out as
/// any null part is.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting the first text part that is not JSON.
pub(crate) fn decode_parts(parts: SourceParts) -> Result<SourceParts, AttributeRefusal> {
    parts
        .into_iter()
        .map(|part| if is_absent(&part) { Ok(JsonValue::Null) } else { json_of(part) })
        .collect()
}

/// Reads a value as a JSON object, or the object its JSON text spells, keeping every null member,
/// for a declaration that requires `required` of it, or `Ok(None)` when the value is absent (see
/// [`is_absent`]).
///
/// # Errors
/// Returns the refusal of a value that is no object (see [`object_of`]).
pub(crate) fn read_object(value: JsonValue, required: RequiredValue) -> Result<Option<JsonObject>, AttributeRefusal> {
    if is_absent(&value) {
        return Ok(None);
    }
    object_of(value, required).map(Some)
}

/// Reads a value the record carries as a JSON object, or the object its JSON text spells, keeping
/// every null member, for a declaration that requires `required` of it.
///
/// An empty object is kept: whether it is a value at all is the declaration's type to decide.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting a value that is no object as not the
/// `required` value: text as written when it is not JSON or is JSON but no object, and any other
/// value as its compact JSON text.
pub(crate) fn object_of(value: JsonValue, required: RequiredValue) -> Result<JsonObject, AttributeRefusal> {
    match value {
        JsonValue::Object(object) => Ok(object),
        JsonValue::String(text) => match parse_json(&text, required)? {
            JsonValue::Object(object) => Ok(object),
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Array(_) => {
                Err(AttributeRefusal::unusable(required, &text))
            }
        },
        other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_)) => {
            Err(AttributeRefusal::unusable(required, &other.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::json_text::{decode_parts, json_of, read_object},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    /// The object a reading kept, as JSON.
    fn object(value: JsonValue) -> Result<Option<JsonValue>, AttributeRefusal> {
        read_object(value, RequiredValue::JsonObject).map(|object| object.map(JsonValue::Object))
    }

    #[test]
    fn reading_an_object_keeps_its_null_members() {
        assert_eq!(object(json!({"a": 1, "b": null})), Ok(Some(json!({"a": 1, "b": null}))));
    }

    #[test]
    fn reading_an_object_from_its_text_keeps_its_null_members() {
        assert_eq!(object(json!(r#"{"a":null}"#)), Ok(Some(json!({"a": null}))));
    }

    #[test]
    fn reading_an_object_refuses_a_value_that_is_no_object() {
        assert_eq!(object(json!([1])), Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "[1]")));
        assert_eq!(object(json!("nope")), Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "nope")));
    }

    #[test]
    fn reading_an_object_from_blank_text_reads_nothing() {
        assert_eq!(object(json!("  ")), Ok(None));
    }

    #[test]
    fn reading_an_empty_object_keeps_it() {
        assert_eq!(object(json!({})), Ok(Some(json!({}))));
    }

    #[test]
    fn reading_a_null_object_reads_nothing() {
        assert_eq!(object(JsonValue::Null), Ok(None));
    }

    #[test]
    fn reading_an_object_refuses_text_spelling_null() {
        assert_eq!(object(json!("null")), Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "null")));
    }

    #[test]
    fn reading_json_reads_text_as_the_json_it_spells_and_keeps_its_nulls() {
        assert_eq!(json_of(json!(r#"{"x":null}"#)), Ok(json!({"x": null})));
        assert_eq!(json_of(json!("12.5")), Ok(json!(12.5)));
        assert_eq!(json_of(json!("null")), Ok(JsonValue::Null));
        assert_eq!(json_of(json!([1, null])), Ok(json!([1, null])));
        assert_eq!(json_of(json!("nope")), Err(AttributeRefusal::unusable(RequiredValue::Json, "nope")));
    }

    #[test]
    fn decoding_parts_turns_an_absent_part_into_a_null_in_its_position() {
        assert_eq!(
            decode_parts(smallvec![json!(""), json!("[1]"), JsonValue::Null, json!("null")]),
            Ok(smallvec![JsonValue::Null, json!([1]), JsonValue::Null, JsonValue::Null])
        );
    }
}

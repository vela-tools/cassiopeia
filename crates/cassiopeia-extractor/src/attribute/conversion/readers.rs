use crate::{attribute::refusal::AttributeRefusal, required_value::RequiredValue};
use cassiopeia_ngsi_ld::value::{json_payload::JsonObject, reported_text::is_blank};
use serde_json::Value as JsonValue;
use smallvec::SmallVec;

/// The raw source values read for one attribute declaration, one per source template.
///
/// A declaration names a single source in the overwhelmingly common case, so the inline capacity of
/// one keeps every leaf, language entry, instance, and metadata sub-attribute of every record off the
/// heap, where a `Vec` would cost one malloc/free pair per resolved declaration.
pub(crate) type SourceParts = SmallVec<[JsonValue; 1]>;

/// Whether a source part is a field the record does not carry: null, or blank text (see
/// [`is_blank`]).
pub(crate) fn is_absent(part: &JsonValue) -> bool {
    match part {
        JsonValue::Null => true,
        JsonValue::String(text) => is_blank(text),
        JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => false,
    }
}

/// Takes a lone part that is not null, the shape every merge keeps a single part in.
fn lone(mut parts: SourceParts) -> Option<JsonValue> {
    Some(parts.swap_remove(0)).filter(|part| !part.is_null())
}

/// Merges parts for a scalar reading: a lone part is kept as it is, and of several the first that
/// is not null is taken.
pub(crate) fn merge_first(parts: SourceParts) -> Option<JsonValue> {
    if parts.len() == 1 {
        return lone(parts);
    }

    parts.into_iter().find(|part| !part.is_null())
}

/// Merges parts for a text reading: a lone part is kept as it is, and several are concatenated into
/// one text, each text part as written and any other part as its compact JSON text.
///
/// A part that is null is a field the record does not carry and contributes nothing; when nothing is
/// left, the parts carry no value.
pub(crate) fn merge_text(parts: SourceParts) -> Option<JsonValue> {
    if parts.len() == 1 {
        return lone(parts);
    }

    let combined = parts
        .iter()
        .filter(|part| !part.is_null())
        .map(|part| match part {
            JsonValue::String(text) => text.clone(),
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => part.to_string(),
        })
        .collect::<String>();

    if combined.is_empty() { None } else { Some(JsonValue::String(combined)) }
}

/// Merges parts for a geometry reading: a lone part is kept as it is, and several keep their array
/// structure, so their order, the coordinate order, survives.
///
/// A part that is null is a field the record does not carry, not a null inside the source's own
/// data, so it is left out of the array.
pub(crate) fn merge_structured(parts: SourceParts) -> Option<JsonValue> {
    if parts.len() == 1 {
        return lone(parts);
    }

    let present = parts.into_iter().filter(|part| !part.is_null()).collect::<Vec<JsonValue>>();
    if present.is_empty() { None } else { Some(JsonValue::Array(present)) }
}

/// Keeps the parts as the source held them: a lone part that is present is its own value, and
/// several present parts are an array of them in order.
///
/// Nothing is coerced, stringified, or filtered by JSON type, and a null nested in a part is kept. A
/// part that is null is a field the record does not carry and is left out; when no part is left
/// there is no value.
pub(crate) fn keep_as_read(parts: SourceParts) -> Option<JsonValue> {
    let mut present = parts.into_iter().filter(|part| !part.is_null()).collect::<SourceParts>();
    match present.len() {
        0 => None,
        1 => Some(present.swap_remove(0)),
        _ => Some(JsonValue::Array(present.into_vec())),
    }
}

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

/// Flattens the parts into one list, splicing in every array and keeping every null item.
///
/// A part that is absent (see [`is_absent`]) contributes nothing: keeping a blank would emit `[""]`
/// for an empty cell. A text part that reads as a JSON array is spliced in like an array part, since
/// a CSV source can carry an array only as its text; any other text, such as `BS IN`, is one item.
/// Only whole parts are judged that way: the items of an array part are the source's own data and
/// are spliced in as they are, nulls included, for the declaration's type to admit. An empty list
/// carries no value.
pub(crate) fn aggregate(parts: SourceParts) -> Option<Vec<JsonValue>> {
    let mut items = Vec::new();
    for part in parts {
        match part {
            JsonValue::Array(array) => items.extend(array),
            JsonValue::Null => {}
            JsonValue::String(text) if is_blank(&text) => {}
            JsonValue::String(text) => match serde_json::from_str::<JsonValue>(&text).ok() {
                Some(JsonValue::Array(array)) => items.extend(array),
                Some(JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Object(_)) | None => {
                    items.push(JsonValue::String(text));
                }
            },
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Object(_) => items.push(part),
        }
    }

    if items.is_empty() { None } else { Some(items) }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::readers::{aggregate, decode_parts, json_of, read_object},
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
    fn aggregating_keeps_null_items_and_splices_array_text() {
        assert_eq!(
            aggregate(smallvec![json!("[1,null]"), json!(2)]),
            Some(vec![json!(1), JsonValue::Null, json!(2)])
        );
    }

    #[test]
    fn aggregating_absent_parts_reads_nothing() {
        assert_eq!(aggregate(smallvec![JsonValue::Null, json!("")]), None);
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

    #[test]
    fn reading_a_null_object_reads_nothing() {
        assert_eq!(object(JsonValue::Null), Ok(None));
    }

    #[test]
    fn reading_an_object_refuses_text_spelling_null() {
        assert_eq!(object(json!("null")), Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "null")));
    }
}

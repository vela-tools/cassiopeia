use crate::attribute::conversion::source_parts::{SourceParts, is_absent};
use cassiopeia_mapping::conversion::value_reading::ValueReading;
use cassiopeia_ngsi_ld::value::reported_text::is_blank;
use serde_json::Value as JsonValue;

/// How the parts of a declaration merge into the one value a reading reads.
///
/// A lone part is kept as it is under every merge; the merges differ in what several parts become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartMerge {
    /// Of several parts, the first that is not null, as a scalar reading takes it.
    First,
    /// Several parts concatenated into one text, each text part as written and any other part as
    /// its compact JSON text, as the text, temporal, identifier, object, and JSON readings take them.
    Text,
    /// Several parts kept in their array structure, so their order, the coordinate order, survives,
    /// as a geometry reading takes them.
    Structured,
}

/// The merge a reading applies to the parts it reads.
// Manual because which merge applies is decided per reading, which no derive expresses.
impl From<ValueReading> for PartMerge {
    fn from(reading: ValueReading) -> PartMerge {
        match reading {
            ValueReading::Boolean | ValueReading::Integer | ValueReading::Float => PartMerge::First,
            ValueReading::Text | ValueReading::Temporal(_) | ValueReading::Identifier(_) | ValueReading::Object | ValueReading::Json => PartMerge::Text,
            ValueReading::Geometry(_) => PartMerge::Structured,
        }
    }
}

impl PartMerge {
    /// Merges the parts into one value, or `None` when they carry no value.
    ///
    /// A part that is null is a field the record does not carry and contributes nothing: of several
    /// it is skipped, left out of the text, or left out of the array, and a lone one is no value.
    /// Text merged from nothing is no value either. A structured merge carries no value when every
    /// part is absent (see [`is_absent`]), as an empty cell, or a pair of empty coordinate cells, is,
    /// since no geometry is written as blank text.
    pub(crate) fn merge(self, parts: SourceParts) -> Option<JsonValue> {
        match self {
            PartMerge::Structured if parts.iter().all(is_absent) => None,
            PartMerge::First | PartMerge::Text | PartMerge::Structured if parts.len() == 1 => lone(parts),
            PartMerge::First => parts.into_iter().find(|part| !part.is_null()),
            PartMerge::Text => concatenated(parts),
            PartMerge::Structured => Some(JsonValue::Array(parts.into_iter().filter(|part| !part.is_null()).collect())),
        }
    }
}

/// Takes a lone part that is not null, the shape every merge keeps a single part in.
fn lone(mut parts: SourceParts) -> Option<JsonValue> {
    Some(parts.swap_remove(0)).filter(|part| !part.is_null())
}

/// Concatenates the parts that are not null into one text, each text part as written and any other
/// part as its compact JSON text, or `None` when nothing is left.
fn concatenated(parts: SourceParts) -> Option<JsonValue> {
    let combined = parts
        .into_iter()
        .filter(|part| !part.is_null())
        .map(|part| match part {
            JsonValue::String(text) => text,
            other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => other.to_string(),
        })
        .collect::<String>();

    if combined.is_empty() { None } else { Some(JsonValue::String(combined)) }
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
    use crate::attribute::conversion::part_merge::{PartMerge, aggregate, keep_as_read};
    use cassiopeia_geometry::target::GeometryTarget;
    use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
    use cassiopeia_ngsi_ld::value::types::TemporalValueType;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    #[test]
    fn each_reading_names_the_merge_it_applies() {
        for (reading, merge) in [
            (ValueReading::Boolean, PartMerge::First),
            (ValueReading::Integer, PartMerge::First),
            (ValueReading::Float, PartMerge::First),
            (ValueReading::Text, PartMerge::Text),
            (ValueReading::Temporal(TemporalValueType::DateTime), PartMerge::Text),
            (ValueReading::Identifier(IdentifierReading::Urn), PartMerge::Text),
            (ValueReading::Object, PartMerge::Text),
            (ValueReading::Json, PartMerge::Text),
            (ValueReading::Geometry(GeometryTarget::Preserve), PartMerge::Structured),
        ] {
            assert_eq!(PartMerge::from(reading), merge, "{reading:?}");
        }
    }

    #[test]
    fn a_lone_part_is_kept_as_it_is_and_a_lone_null_is_no_value() {
        for merge in [PartMerge::First, PartMerge::Text, PartMerge::Structured] {
            assert_eq!(merge.merge(smallvec![json!([1])]), Some(json!([1])), "{merge:?}");
            assert_eq!(merge.merge(smallvec![JsonValue::Null]), None, "{merge:?}");
        }
    }

    #[test]
    fn the_first_merge_takes_the_first_part_that_is_not_null() {
        assert_eq!(PartMerge::First.merge(smallvec![JsonValue::Null, json!(""), json!(7)]), Some(json!("")));
        assert_eq!(PartMerge::First.merge(smallvec![JsonValue::Null, JsonValue::Null]), None);
    }

    #[test]
    fn the_text_merge_concatenates_the_parts_that_are_not_null() {
        assert_eq!(
            PartMerge::Text.merge(smallvec![json!("a-"), JsonValue::Null, json!(1), json!([true])]),
            Some(json!("a-1[true]"))
        );
        assert_eq!(PartMerge::Text.merge(smallvec![JsonValue::Null, json!("")]), None);
    }

    #[test]
    fn the_structured_merge_keeps_the_parts_that_are_not_null_in_order() {
        assert_eq!(
            PartMerge::Structured.merge(smallvec![json!("14.5"), JsonValue::Null, json!(46.0)]),
            Some(json!(["14.5", 46.0]))
        );
        assert_eq!(PartMerge::Structured.merge(smallvec![json!(""), json!("46.0")]), Some(json!(["", "46.0"])));
    }

    #[test]
    fn the_structured_merge_of_only_absent_parts_is_no_value() {
        assert_eq!(PartMerge::Structured.merge(smallvec![json!(""), JsonValue::Null]), None);
        assert_eq!(PartMerge::Structured.merge(smallvec![json!("  ")]), None);
    }

    #[test]
    fn keeping_as_read_leaves_out_null_parts_and_keeps_nested_nulls() {
        assert_eq!(keep_as_read(smallvec![JsonValue::Null, json!({"a": null})]), Some(json!({"a": null})));
        assert_eq!(keep_as_read(smallvec![json!(1), JsonValue::Null, json!("x")]), Some(json!([1, "x"])));
        assert_eq!(keep_as_read(smallvec![JsonValue::Null]), None);
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
}

use cassiopeia_ngsi_ld::value::reported_text::is_blank;
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

#[cfg(test)]
mod tests {
    use crate::attribute::conversion::source_parts::is_absent;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn a_null_or_blank_part_is_absent() {
        for part in [JsonValue::Null, json!(""), json!(" \t")] {
            assert!(is_absent(&part), "{part}");
        }
    }

    #[test]
    fn any_other_part_is_present() {
        for part in [json!("a"), json!(false), json!(0), json!([]), json!({})] {
            assert!(!is_absent(&part), "{part}");
        }
    }
}

use derive_more::Display;
use serde_json::Value as JsonValue;

/// The field reference whose lone use as an attribute source takes the whole record as the value.
const WHOLE_RECORD: &str = "context";

/// A dot-separated path naming a source field, such as `properties.tipo`.
///
/// A newtype rather than a bare `String` so a field reference cannot be confused with literal
/// template text or an unrelated string; it owns the navigation logic that reads the field it names
/// out of a source record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[display("{_0}")]
pub struct FieldPath(String);

impl FieldPath {
    /// Wraps a dot-separated field path.
    #[must_use]
    pub fn new(path: impl Into<String>) -> FieldPath {
        FieldPath(path.into())
    }

    /// The path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The top-level key the path starts at.
    #[must_use]
    pub fn head(&self) -> &str {
        self.0.split_once('.').map_or(self.0.as_str(), |(head, _rest)| head)
    }

    /// Whether this is the reference an attribute source writes, alone, to take the whole record as
    /// its value rather than one field of it.
    #[must_use]
    pub fn is_whole_record(&self) -> bool {
        self.0 == WHOLE_RECORD
    }

    /// Reads the value this path names out of a source record.
    ///
    /// A missing key, or a path that runs through a non-object, yields [`JsonValue::Null`] rather
    /// than an error: absence of a source field is normal for sparse records and is handled
    /// downstream by null-skipping rather than by failing the record.
    #[must_use]
    pub fn read(&self, data: &JsonValue) -> JsonValue {
        self.0
            .split('.')
            .try_fold(data, |current, segment| current.as_object()?.get(segment))
            .cloned()
            .unwrap_or(JsonValue::Null)
    }
}

#[cfg(test)]
mod tests {
    use crate::template::field_path::FieldPath;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn reads_a_top_level_key() {
        assert_eq!(FieldPath::new("id").read(&json!({"id": 7})), json!(7));
    }

    #[test]
    fn walks_a_nested_path() {
        let data = json!({"properties": {"tipo": "sensor"}});

        assert_eq!(FieldPath::new("properties.tipo").read(&data), json!("sensor"));
    }

    #[test]
    fn a_missing_key_reads_as_null() {
        assert_eq!(FieldPath::new("name").read(&json!({"id": 7})), JsonValue::Null);
    }

    #[test]
    fn a_path_through_a_non_object_reads_as_null() {
        assert_eq!(FieldPath::new("id.nested").read(&json!({"id": 7})), JsonValue::Null);
    }

    #[test]
    fn a_nested_object_is_read_whole() {
        let data = json!({"address": {"city": "Ljubljana"}});

        assert_eq!(FieldPath::new("address").read(&data), json!({"city": "Ljubljana"}));
    }

    #[test]
    fn the_head_is_the_first_segment_and_a_single_key_is_its_own_head() {
        assert_eq!(FieldPath::new("properties.tipo").head(), "properties");
        assert_eq!(FieldPath::new("id").head(), "id");
    }

    #[test]
    fn only_the_context_reference_is_the_whole_record() {
        assert!(FieldPath::new("context").is_whole_record());
        assert!(!FieldPath::new("context.id").is_whole_record());
    }

    #[test]
    fn displays_as_its_path() {
        assert_eq!(FieldPath::new("properties.tipo").to_string(), "properties.tipo");
    }
}

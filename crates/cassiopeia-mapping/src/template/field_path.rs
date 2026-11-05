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
        self.lookup(data).cloned().unwrap_or(JsonValue::Null)
    }

    /// The value this path names in a source record, or `None` when the record does not have it: a
    /// key is missing, or the path runs through a non-object.
    #[must_use]
    pub fn lookup<'a>(&self, data: &'a JsonValue) -> Option<&'a JsonValue> {
        self.0.split('.').try_fold(data, |current, segment| current.as_object()?.get(segment))
    }

    /// The shortest leading part of this path that a source record does not have, or `None` when
    /// it has the whole path.
    ///
    /// For `address.street.name` over a record whose `address` has no `street`, that is
    /// `address.street`. A segment beneath a value that is not an object, null included, is one the
    /// record does not have.
    #[must_use]
    pub fn missing_prefix(&self, data: &JsonValue) -> Option<FieldPath> {
        let mut current = data;
        for (end, segment) in self.segment_ends() {
            match current.as_object().and_then(|fields| fields.get(segment)) {
                Some(value) => current = value,
                None => return Some(FieldPath::new(&self.0[..end])),
            }
        }
        None
    }

    /// The last segment of the path: `name` in `address.street.name`.
    #[must_use]
    pub fn last_segment(&self) -> &str {
        self.0.rsplit_once('.').map_or(self.0.as_str(), |(_rest, last)| last)
    }

    /// Each segment of the path with the byte offset just past it.
    fn segment_ends(&self) -> impl Iterator<Item = (usize, &str)> {
        self.0.split('.').scan(0, |start, segment| {
            let end = *start + segment.len();
            *start = end + 1;
            Some((end, segment))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::template::field_path::FieldPath;
    use pretty_assertions::assert_eq;
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
    fn a_present_value_is_looked_up_and_a_missing_one_is_none() {
        let data = json!({"address": {"city": null}, "id": 7});

        assert_eq!(FieldPath::new("address.city").lookup(&data), Some(&JsonValue::Null));
        assert_eq!(FieldPath::new("address.street").lookup(&data), None);
        assert_eq!(FieldPath::new("id.nested").lookup(&data), None);
    }

    #[test]
    fn the_missing_prefix_ends_at_the_first_segment_the_record_lacks() {
        let data = json!({"address": {"city": {}}, "id": 7, "gone": null});

        assert_eq!(
            FieldPath::new("address.street.name").missing_prefix(&data),
            Some(FieldPath::new("address.street"))
        );
        assert_eq!(
            FieldPath::new("address.city.name").missing_prefix(&data),
            Some(FieldPath::new("address.city.name"))
        );
        assert_eq!(FieldPath::new("name").missing_prefix(&data), Some(FieldPath::new("name")));
    }

    #[test]
    fn a_segment_beneath_a_non_object_is_missing() {
        let data = json!({"id": 7, "gone": null});

        assert_eq!(FieldPath::new("id.nested").missing_prefix(&data), Some(FieldPath::new("id.nested")));
        assert_eq!(FieldPath::new("gone.nested.deeper").missing_prefix(&data), Some(FieldPath::new("gone.nested")));
    }

    #[test]
    fn a_path_the_record_has_has_no_missing_prefix_even_when_its_value_is_null() {
        let data = json!({"address": {"city": null}});

        assert_eq!(FieldPath::new("address.city").missing_prefix(&data), None);
        assert_eq!(FieldPath::new("address").missing_prefix(&data), None);
    }

    #[test]
    fn the_last_segment_is_the_name_after_the_last_dot() {
        assert_eq!(FieldPath::new("address.street.name").last_segment(), "name");
        assert_eq!(FieldPath::new("id").last_segment(), "id");
    }

    #[test]
    fn displays_as_its_path() {
        assert_eq!(FieldPath::new("properties.tipo").to_string(), "properties.tipo");
    }
}

use serde_json::{Map, Value};

/// A served value as a record holds it: absent when it is `null`, and without `null` members.
///
/// NGSI-LD admits no `null` (ETSI GS CIM 009 v1.9.1 clause 4.5.0), and a template that reads an
/// absent member resolves to nothing, so a member a server sends as `null` is left out, at any depth.
/// A `null` item of an array keeps its place, since the positions of an array carry its meaning.
pub(super) fn present(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        other @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => Some(without_nested_nulls(other)),
    }
}

/// Moves the member `served` of `source` into `target` as `key`, when it is present.
pub(super) fn move_member(target: &mut Map<String, Value>, key: &str, source: &mut Map<String, Value>, served: &str) {
    if let Some(value) = source.remove(served).and_then(present) {
        target.insert(key.to_owned(), value);
    }
}

/// `value` with its `null` members removed at any depth.
fn without_nested_nulls(value: Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .filter_map(|(key, member)| present(member).map(|member| (key, member)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(without_nested_nulls).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::null_members::{move_member, present};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    #[test]
    fn a_null_value_is_absent() {
        assert_eq!(present(Value::Null), None);
    }

    #[test]
    fn null_members_are_removed_at_any_depth() {
        assert_eq!(
            present(json!({"symbol": "ug/m3", "name": null, "nested": {"a": null, "b": 1}})),
            Some(json!({"symbol": "ug/m3", "nested": {"b": 1}}))
        );
    }

    #[test]
    fn a_null_array_item_keeps_its_place() {
        assert_eq!(present(json!([1, null, {"a": null}])), Some(json!([1, null, {}])));
    }

    #[test]
    fn a_member_is_moved_under_its_record_key_when_present() {
        let Value::Object(mut source) = json!({"@iot.id": 5, "description": null}) else {
            panic!("expected an object");
        };
        let mut target = Map::new();

        move_member(&mut target, "id", &mut source, "@iot.id");
        move_member(&mut target, "description", &mut source, "description");

        assert_eq!(Value::Object(target), json!({"id": 5}));
        assert!(source.is_empty());
    }
}

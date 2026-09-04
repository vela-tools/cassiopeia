use serde_json::{Map, Value};

/// Outcome of scanning a folded object for the single array-valued member that drives record splitting.
pub enum LoneArrayMember {
    /// Exactly one candidate member held an array; its key is carried here.
    Found(String),
    /// Zero members, or two or more, held an array: the object does not split.
    NoneOrMany,
}

/// Finds the single array-valued member among the keys accepted by `is_candidate`.
///
/// Two-plus array members deliberately decline to pick a winner, matching the schema-less
/// "convention, not guessing" rule shared by the JSON and XML ingestors.
pub fn lone_array_member(map: &Map<String, Value>, mut is_candidate: impl FnMut(&str) -> bool) -> LoneArrayMember {
    let mut array_members = map.iter().filter(|(key, value)| is_candidate(key) && value.is_array());
    let first = array_members.next().map(|(key, _)| key.clone());
    let has_second = array_members.next().is_some();

    match (first, has_second) {
        (Some(key), false) => LoneArrayMember::Found(key),
        (Some(_) | None, true) | (None, false) => LoneArrayMember::NoneOrMany,
    }
}

#[cfg(test)]
mod tests {
    use crate::lone_array::{LoneArrayMember, lone_array_member};
    use serde_json::{Map, Value, json};

    fn map(value: Value) -> Map<String, Value> {
        let Value::Object(map) = value else {
            panic!("expected an object");
        };
        map
    }

    #[test]
    fn exactly_one_array_member_is_found() {
        let object = map(json!({ "meta": { "n": 2 }, "data": [{ "a": 1 }] }));
        assert!(matches!(lone_array_member(&object, |_| true), LoneArrayMember::Found(key) if key == "data"));
    }

    #[test]
    fn no_array_member_is_none_or_many() {
        let object = map(json!({ "a": 1, "b": 2 }));
        assert!(matches!(lone_array_member(&object, |_| true), LoneArrayMember::NoneOrMany));
    }

    #[test]
    fn two_array_members_is_none_or_many() {
        let object = map(json!({ "a": [{ "x": 1 }], "b": [{ "y": 2 }] }));
        assert!(matches!(lone_array_member(&object, |_| true), LoneArrayMember::NoneOrMany));
    }

    #[test]
    fn the_candidate_predicate_filters_out_rejected_keys() {
        let object = map(json!({ "@attr": [1], "child": [{ "a": 1 }] }));
        assert!(matches!(lone_array_member(&object, |key| !key.starts_with('@')), LoneArrayMember::Found(key) if key == "child"));
    }
}

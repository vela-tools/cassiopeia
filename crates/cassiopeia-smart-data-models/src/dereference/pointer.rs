use crate::error::{Result, SdmError};
use serde_json::Value;

/// Resolves the fragment part of a `$ref` against the document it points into.
///
/// Implements RFC 6901: the empty pointer is the whole document, every other pointer is a sequence
/// of `/`-separated tokens in which `~1` stands for `/` and `~0` for `~`.
///
/// # Errors
/// Returns [`SdmError::InvalidJsonPointer`] when `pointer` is not a valid RFC 6901 pointer, and
/// [`SdmError::UnresolvableJsonPointer`] when it names nothing in `document`.
pub fn resolve<'a>(document: &'a Value, pointer: &str) -> Result<&'a Value> {
    if pointer.is_empty() {
        return Ok(document);
    }

    let Some(tokens) = pointer.strip_prefix('/') else {
        return Err(SdmError::InvalidJsonPointer { pointer: pointer.to_string() });
    };

    tokens.split('/').try_fold(document, |current, token| {
        let token = unescape(token);

        match current {
            Value::Object(members) => members.get(&token),
            Value::Array(items) => token.parse().ok().and_then(|index: usize| items.get(index)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
        }
        .ok_or_else(|| SdmError::UnresolvableJsonPointer { pointer: pointer.to_string() })
    })
}

/// Turns the two escape sequences RFC 6901 defines back into the characters they stand for.
fn unescape(token: &str) -> String {
    if token.contains('~') {
        token.replace("~1", "/").replace("~0", "~")
    } else {
        token.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::dereference::pointer::resolve;
    use serde_json::json;

    #[test]
    fn the_empty_pointer_is_the_whole_document() {
        let document = json!({"a": 1});

        assert_eq!(resolve(&document, "").unwrap(), &document);
    }

    #[test]
    fn a_pointer_walks_objects_and_arrays() {
        let document = json!({"definitions": {"Sensor": [{"type": "object"}]}});

        assert_eq!(resolve(&document, "/definitions/Sensor/0/type").unwrap(), &json!("object"));
    }

    #[test]
    fn an_escaped_token_names_a_key_containing_a_slash() {
        let document = json!({"a/b": {"~c": 1}});

        assert_eq!(resolve(&document, "/a~1b/~0c").unwrap(), &json!(1));
    }

    #[test]
    fn a_pointer_that_does_not_start_with_a_slash_is_rejected() {
        assert!(resolve(&json!({"a": 1}), "a").is_err());
    }

    #[test]
    fn a_pointer_into_a_missing_key_is_rejected() {
        assert!(resolve(&json!({"a": 1}), "/b").is_err());
    }

    #[test]
    fn a_pointer_into_a_scalar_is_rejected() {
        assert!(resolve(&json!({"a": 1}), "/a/b").is_err());
    }

    #[test]
    fn an_array_index_past_the_end_is_rejected() {
        assert!(resolve(&json!({"a": [1]}), "/a/5").is_err());
    }
}

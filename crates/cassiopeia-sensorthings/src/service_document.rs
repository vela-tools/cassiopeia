use crate::stream_kind::StreamKind;
use serde_json::{Map, Value};

/// The member of an item of a service root document that names the collection it lists.
const NAME: &str = "name";

/// Whether `items`, the items of a server's service root document, list the collection of `kind`.
///
/// A service root document holds one item per collection the server serves, each naming it by its
/// `name` (OGC 18-088 section 9.2.1), so a server without the `MultiDatastream` extension (section
/// 12) lists no `MultiDatastreams`.
#[must_use]
pub fn lists_collection(items: &[Map<String, Value>], kind: StreamKind) -> bool {
    items.iter().any(|item| item.get(NAME).and_then(Value::as_str) == Some(kind.collection()))
}

#[cfg(test)]
mod tests {
    use crate::{service_document::lists_collection, stream_kind::StreamKind};
    use serde_json::{Map, Value, json};

    fn items(value: Value) -> Vec<Map<String, Value>> {
        let Value::Array(items) = value else {
            panic!("expected an array");
        };
        items
            .into_iter()
            .map(|item| match item {
                Value::Object(item) => item,
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => panic!("expected an object"),
            })
            .collect()
    }

    #[test]
    fn a_root_that_names_the_collection_lists_it() {
        let served = items(json!([
            {"name": "Datastreams", "url": "https://h.example/v1.1/Datastreams"},
            {"name": "MultiDatastreams", "url": "https://h.example/v1.1/MultiDatastreams"},
        ]));

        assert!(lists_collection(&served, StreamKind::MultiDatastream));
        assert!(lists_collection(&served, StreamKind::Datastream));
    }

    #[test]
    fn a_root_without_the_extension_lists_no_multi_datastreams() {
        let served = items(json!([
            {"name": "Datastreams", "url": "https://h.example/v1.1/Datastreams"},
            {"name": "multidatastreams"},
            {"url": "https://h.example/v1.1/MultiDatastreams"},
        ]));

        assert!(!lists_collection(&served, StreamKind::MultiDatastream));
    }
}

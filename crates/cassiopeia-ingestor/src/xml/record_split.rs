use crate::lone_array::{LoneArrayMember, lone_array_member};
use cassiopeia_ir::record::Record;
use serde_json::{Map, Value};

/// Splits a folded XML document into records under Cassiopeia's convention.
///
/// The rule is deterministic and schema-less, mirroring JSON's refusal to guess: the document is
/// always one collection (`collection == None` on every record), and records come from a single
/// repeated child element under the root, never from a heuristic search.
///
/// - Root object with exactly one `Array`-valued element-child (a child element name repeated in the
///   source): its items are the records. An object item becomes the record's data; a leaf item is
///   wrapped as `{ "#text": item }`.
/// - Root object with zero, or two-or-more, `Array`-valued element-children: the whole root object is
///   a single record. The two-plus case deliberately declines to pick a winner.
/// - Leaf root (`<root>text</root>` or `<root/>`): one record keyed by the root element name.
///
/// Accepted limitations, all a consequence of "convention, not guessing" (reshape the source to work
/// within them): a single-occurrence child does not split (a one-station feed becomes one record
/// spanning the whole root); a repeated element nested under a wrapper element is addressable but not
/// split; and a node with two repeating children stays one record.
#[must_use]
pub fn split(document: Value) -> Vec<Record> {
    let Some((root_name, root_value)) = root_entry(document) else {
        return Vec::new();
    };
    match root_value {
        Value::Object(root_map) => split_object_root(root_map),
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null | Value::Array(_) => {
            // A leaf root (an array cannot occur at the document level, but is grouped here for
            // exhaustiveness): one record keyed by the root element name.
            let mut data = Map::new();
            data.insert(root_name, root_value);
            vec![Record::new(None, data)]
        }
    }
}

/// Extracts the single root element entry (the one key that is neither an attribute nor a metadata key).
fn root_entry(document: Value) -> Option<(String, Value)> {
    match document {
        Value::Object(map) => map.into_iter().find(|(key, _)| is_element_key(key)),
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null | Value::Array(_) => None,
    }
}

/// Applies the convention to an object-valued root: split on a lone repeated child, else one record.
fn split_object_root(mut root_map: Map<String, Value>) -> Vec<Record> {
    match lone_array_member(&root_map, is_element_key) {
        LoneArrayMember::Found(key) => match root_map.remove(&key) {
            Some(Value::Array(items)) => items.into_iter().map(item_to_record).collect(),
            Some(Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null | Value::Object(_)) | None => {
                // Unreachable: `key` named an array element-child a moment ago.
                vec![Record::new(None, root_map)]
            }
        },
        LoneArrayMember::NoneOrMany => vec![Record::new(None, root_map)],
    }
}

/// Turns one array item into a record: an object supplies its data directly, a leaf is wrapped as `#text`.
fn item_to_record(item: Value) -> Record {
    match item {
        Value::Object(map) => Record::new(None, map),
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null | Value::Array(_) => {
            let mut data = Map::new();
            data.insert("#text".to_owned(), item);
            Record::new(None, data)
        }
    }
}

/// Reports whether a key names a child element, as opposed to an attribute (`@`) or metadata (`#`).
fn is_element_key(key: &str) -> bool {
    !key.starts_with('@') && !key.starts_with('#')
}

#[cfg(test)]
mod tests {
    use crate::xml::{record_split::split, value::document_to_value};
    use cassiopeia_ir::record::Record;
    use serde_json::{Value, json};

    fn split_xml(xml: &[u8]) -> Vec<Record> {
        split(document_to_value(xml).unwrap())
    }

    #[test]
    fn a_root_with_a_repeated_child_yields_one_record_per_occurrence() {
        let records = split_xml(b"<data><metData><t>1</t></metData><metData><t>2</t></metData><metData><t>3</t></metData></data>");
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.collection().is_none()));
        assert_eq!(records[0].data().get("t"), Some(&Value::String("1".to_owned())));
    }

    #[test]
    fn a_single_occurrence_child_does_not_split_and_the_whole_root_is_one_record() {
        let records = split_xml(b"<data><title>Feed</title><metData><t>24</t></metData></data>");
        assert_eq!(records.len(), 1);
        // The field lives one level deeper than the many-case: metData.t, not t.
        let metdata = records[0].data().get("metData").unwrap();
        assert_eq!(metdata, &json!({ "t": "24" }));
    }

    #[test]
    fn a_repeated_grandchild_under_a_wrapper_does_not_split() {
        let records = split_xml(b"<data><stations><metData>1</metData><metData>2</metData><metData>3</metData></stations></data>");
        assert_eq!(records.len(), 1);
        let stations = records[0].data().get("stations").unwrap();
        assert_eq!(stations, &json!({ "metData": ["1", "2", "3"] }));
    }

    #[test]
    fn a_root_with_two_repeated_children_declines_to_pick_and_stays_one_record() {
        let records = split_xml(b"<data><a>1</a><a>2</a><b>3</b><b>4</b></data>");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("a"), Some(&json!(["1", "2"])));
        assert_eq!(records[0].data().get("b"), Some(&json!(["3", "4"])));
    }

    #[test]
    fn an_all_scalar_root_object_is_one_record() {
        let records = split_xml(b"<data><title>Feed</title><count>3</count></data>");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("title"), Some(&Value::String("Feed".to_owned())));
    }

    #[test]
    fn a_leaf_root_is_one_record_keyed_by_the_root_name() {
        let records = split_xml(b"<root>text</root>");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("root"), Some(&Value::String("text".to_owned())));
    }

    #[test]
    fn an_empty_root_is_one_record_with_a_null_value() {
        let records = split_xml(b"<root/>");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("root"), Some(&Value::Null));
    }

    #[test]
    fn repeated_text_leaves_are_each_wrapped_under_text() {
        let records = split_xml(b"<data><item>a</item><item>b</item></data>");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("#text"), Some(&Value::String("a".to_owned())));
        assert_eq!(records[1].data().get("#text"), Some(&Value::String("b".to_owned())));
    }
}

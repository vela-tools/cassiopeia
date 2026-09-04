use crate::{
    json::error::JsonIngestError,
    lone_array::{LoneArrayMember, lone_array_member},
};
use cassiopeia_ir::record::Record;
use serde_json::{Map, Value};
use std::vec;

/// Records selected from one parsed JSON document.
///
/// Array and envelope members remain in their original allocation and are converted into records as
/// the ingestor fills each output batch. This avoids materializing a second document-sized record
/// vector while retaining the input order.
pub struct JsonRecords {
    source: RecordSource,
}

/// The one-record and many-record shapes accepted by the lone-array convention.
enum RecordSource {
    Single(Option<Map<String, Value>>),
    Multiple(vec::IntoIter<Value>),
}

impl JsonRecords {
    /// Creates a one-record stream without allocating an intermediate vector.
    const fn single(record: Map<String, Value>) -> JsonRecords {
        JsonRecords {
            source: RecordSource::Single(Some(record)),
        }
    }

    /// Creates a record stream over an existing JSON array allocation.
    fn multiple(items: Vec<Value>) -> Result<JsonRecords, JsonIngestError> {
        ensure_object_items(&items)?;
        Ok(JsonRecords {
            source: RecordSource::Multiple(items.into_iter()),
        })
    }
}

impl Iterator for JsonRecords {
    type Item = Result<Record, JsonIngestError>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.source {
            RecordSource::Single(record) => record.take().map(|data| Ok(Record::new(None, data))),
            RecordSource::Multiple(items) => items.next().map(object_item),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len();
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for JsonRecords {
    fn len(&self) -> usize {
        match &self.source {
            RecordSource::Single(record) => usize::from(record.is_some()),
            RecordSource::Multiple(items) => items.len(),
        }
    }
}

/// Splits a parsed JSON document into records under the lone-array convention.
///
/// The rule is deterministic and schema-less, mirroring XML's refusal to guess: the document is
/// always one collection (`collection == None` on every record), and the shape decides the records:
///
/// - Array: its items are the records; each item must be an object, else [`JsonIngestError::ExpectedObject`].
/// - Object with exactly one array-valued member: that array's items are the records (envelope case);
///   each item must be an object. The sibling members are dropped.
/// - Object with zero, or two-or-more, array-valued members: the whole object is a single record. The
///   two-plus case deliberately declines to pick a winner.
/// - Scalar or null: [`JsonIngestError::ExpectedObjectOrArray`], as it can be neither record nor records.
///
/// Accepted limitation, a consequence of "convention, not guessing": a single record carrying exactly
/// one array-of-objects field is read as an envelope, dropping its sibling fields, identical to XML.
///
/// # Errors
///
/// Returns [`JsonIngestError`] when a record-bearing element is not an object, or the root is a scalar.
pub fn split(document: Value) -> Result<JsonRecords, JsonIngestError> {
    match document {
        Value::Array(items) => JsonRecords::multiple(items),
        Value::Object(mut map) => match lone_array_member(&map, |_| true) {
            LoneArrayMember::Found(key) => match map.remove(&key) {
                Some(Value::Array(items)) => JsonRecords::multiple(items),
                Some(value) => {
                    // Preserve the document if the earlier immutable scan and removal ever disagree.
                    map.insert(key, value);
                    Ok(JsonRecords::single(map))
                }
                None => Ok(JsonRecords::single(map)),
            },
            LoneArrayMember::NoneOrMany => Ok(JsonRecords::single(map)),
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Err(JsonIngestError::ExpectedObjectOrArray),
    }
}

/// Checks all record-bearing values before any record can be sent downstream.
fn ensure_object_items(items: &[Value]) -> Result<(), JsonIngestError> {
    if items.iter().all(Value::is_object) {
        Ok(())
    } else {
        Err(JsonIngestError::ExpectedObject)
    }
}

/// Turns one array or envelope item into a record, rejecting any item that is not an object.
fn object_item(item: Value) -> Result<Record, JsonIngestError> {
    match item {
        Value::Object(map) => Ok(Record::new(None, map)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Err(JsonIngestError::ExpectedObject),
    }
}

#[cfg(test)]
mod tests {
    use crate::json::{error::JsonIngestError, record_split::split};
    use cassiopeia_ir::record::Record;
    use serde_json::json;

    fn records(document: serde_json::Value) -> Result<Vec<Record>, JsonIngestError> {
        split(document)?.collect()
    }

    #[test]
    fn an_array_of_objects_becomes_one_record_each() {
        let records = records(json!([{ "a": 1 }, { "b": 2 }])).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.collection().is_none()));
    }

    #[test]
    fn a_single_object_with_no_arrays_is_one_record() {
        let records = records(json!({ "a": 1, "b": 2 })).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("a"), Some(&json!(1)));
        assert_eq!(records[0].data().get("b"), Some(&json!(2)));
    }

    #[test]
    fn an_envelope_with_a_lone_data_array_splits_on_it() {
        let records = records(json!({ "meta": { "n": 2 }, "data": [{ "a": 1 }, { "b": 2 }] })).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.data().get("meta").is_none()));
    }

    #[test]
    fn an_object_with_two_array_members_declines_and_stays_one_record() {
        let records = records(json!({ "a": [{ "x": 1 }], "b": [{ "y": 2 }] })).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("a"), Some(&json!([{ "x": 1 }])));
        assert_eq!(records[0].data().get("b"), Some(&json!([{ "y": 2 }])));
    }

    #[test]
    fn an_all_scalar_object_is_one_record() {
        let records = records(json!({ "title": "feed", "count": 3 })).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("title"), Some(&json!("feed")));
    }

    #[test]
    fn a_lone_array_of_objects_field_is_read_as_an_envelope() {
        let records = records(json!({ "id": 1, "readings": [{ "t": 1 }] })).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("t"), Some(&json!(1)));
        assert!(records[0].data().get("id").is_none());
    }

    #[test]
    fn an_empty_envelope_array_yields_no_records() {
        let records = records(json!({ "data": [] })).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn a_non_object_array_element_is_rejected() {
        let result = split(json!([{ "a": 1 }, 5]));
        assert!(matches!(result, Err(JsonIngestError::ExpectedObject)));
    }

    #[test]
    fn a_non_object_envelope_element_is_rejected() {
        let result = split(json!({ "data": [{ "a": 1 }, 5] }));
        assert!(matches!(result, Err(JsonIngestError::ExpectedObject)));
    }

    #[test]
    fn a_scalar_root_is_rejected() {
        assert!(matches!(split(json!(5)), Err(JsonIngestError::ExpectedObjectOrArray)));
        assert!(matches!(split(json!(null)), Err(JsonIngestError::ExpectedObjectOrArray)));
    }
}

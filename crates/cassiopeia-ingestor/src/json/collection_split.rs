use crate::json::error::JsonIngestError;
use cassiopeia_common::collection::CollectionName;
use cassiopeia_ir::record::Record;
use serde_json::{Map, Value};

/// Splits a parsed JSON document into the records of the listed nested collections.
///
/// Every non-empty array whose items are all objects is a collection, labelled with the member
/// names that lead to it from the root, joined with `.` and without array indices:
/// `data.monitors.lines`. The root array has no member path, so it is no collection itself, but the
/// members of its items are walked. An array that holds anything other than objects is a plain
/// value and is not walked.
///
/// A lone object on a listed path counts as a collection of one item. JSON converted from XML writes
/// an element that occurs once as an object and the same element occurring twice as an array, so a
/// collection's shape changes with the number of its items, and both shapes have to yield the same
/// records.
///
/// Each item of a listed collection is handed to `emit` as one record tagged with its label. The
/// record also carries every item that encloses it on the path from the root, under the name of the
/// member that holds that item, so a `departure` item inside a `lines` item can read `lines.name`.
/// An enclosing item comes without its collections and without the members on the listed path,
/// which are records or enclosing items of their own, and an enclosing item left with no fields is
/// not added. On a name clash the record's own field wins, then the nearest enclosing item.
///
/// Only the branches that lead to a listed label are walked; the rest of the document is dropped.
///
/// # Errors
///
/// Returns [`JsonIngestError::ExpectedObjectOrArray`] when the root is a scalar or null, and passes
/// on any error `emit` returns.
pub fn split<E>(document: Value, labels: &[CollectionName], emit: impl FnMut(Record) -> Result<(), E>) -> Result<(), E>
where
    E: From<JsonIngestError>,
{
    let mut walk = CollectionWalk {
        labels,
        path: String::new(),
        enclosing: Vec::new(),
        emit,
    };

    match document {
        Value::Object(map) => walk.object(map),
        Value::Array(items) => items.into_iter().try_for_each(|item| match item {
            Value::Object(map) => walk.object(map),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Ok(()),
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Err(JsonIngestError::ExpectedObjectOrArray.into()),
    }
}

/// An item on the path from the root that encloses the items currently being walked.
struct EnclosingItem {
    /// The member that holds the item, under which the item is added to each inner record.
    name: String,
    /// The item's fields, without its collections and without the members on the listed path.
    fields: Map<String, Value>,
}

/// Where a member path stands relative to the listed collection labels.
enum PathReach<'a> {
    /// No label is this path or lies below it.
    Off,
    /// A label lies below this path, but none is this path.
    Through,
    /// This path is a label, and no label lies below it.
    Label(&'a CollectionName),
    /// This path is a label, and another label lies below it.
    LabelAndThrough(&'a CollectionName),
}

/// The state of one walk through a document.
struct CollectionWalk<'a, F> {
    labels: &'a [CollectionName],
    /// The member path of the value being walked, joined with `.`.
    path: String,
    /// The items around the current position, outermost first.
    enclosing: Vec<EnclosingItem>,
    emit: F,
}

impl<'a, F, E> CollectionWalk<'a, F>
where
    F: FnMut(Record) -> Result<(), E>,
    E: From<JsonIngestError>,
{
    /// Walks every member of an object at the current path.
    fn object(&mut self, map: Map<String, Value>) -> Result<(), E> {
        map.into_iter().try_for_each(|(key, value)| self.member(&key, value))
    }

    /// Walks one member, extending the path with its name while it is walked.
    fn member(&mut self, key: &str, value: Value) -> Result<(), E> {
        let parent_length = self.path.len();
        extend_path(&mut self.path, key);

        let walked = match reach(self.labels, &self.path) {
            PathReach::Off => Ok(()),
            reach @ (PathReach::Through | PathReach::Label(_) | PathReach::LabelAndThrough(_)) => match value {
                Value::Object(map) => self.item(key, map, &reach),
                Value::Array(items) if is_collection(&items) => self.collection(key, items, &reach),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Ok(()),
            },
        };

        self.path.truncate(parent_length);
        walked
    }

    /// Walks the items of the collection at the current path, which `name` holds.
    fn collection(&mut self, name: &str, items: Vec<Value>, reach: &PathReach<'a>) -> Result<(), E> {
        items.into_iter().try_for_each(|item| match item {
            Value::Object(map) => self.item(name, map, reach),
            // `is_collection` has already checked that every item is an object.
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Ok(()),
        })
    }

    /// Emits one item as a record when its path is listed, and walks into it when a listed path lies
    /// below.
    fn item(&mut self, name: &str, item: Map<String, Value>, reach: &PathReach<'a>) -> Result<(), E> {
        match *reach {
            PathReach::Off => Ok(()),
            PathReach::Label(label) => {
                let record = self.record(label, item);
                (self.emit)(record)
            }
            PathReach::Through => self.enclosed(name, item),
            PathReach::LabelAndThrough(label) => {
                // The item is both a record of its own and the source of the records below it, and
                // each needs it whole.
                let record = self.record(label, item.clone());
                (self.emit)(record)?;
                self.enclosed(name, item)
            }
        }
    }

    /// Walks into an item with the item added to the enclosing items of everything below it.
    fn enclosed(&mut self, name: &str, item: Map<String, Value>) -> Result<(), E> {
        let fields = without_walked(self.labels, &self.path, &item);
        self.enclosing.push(EnclosingItem {
            // The enclosing item outlives the borrowed member name it is filed under.
            name: name.to_owned(),
            fields,
        });
        let walked = self.object(item);
        self.enclosing.pop();
        walked
    }

    /// Builds the record for one item of the labelled collection, adding its enclosing items.
    fn record(&self, label: &CollectionName, mut data: Map<String, Value>) -> Record {
        for enclosing in self.enclosing.iter().rev() {
            if !enclosing.fields.is_empty() && !data.contains_key(&enclosing.name) {
                // Every record owns its data, so each one takes its own copy of the enclosing item.
                data.insert(enclosing.name.clone(), Value::Object(enclosing.fields.clone()));
            }
        }
        // Every record owns its collection label.
        Record::new(Some(label.clone()), data)
    }
}

/// Appends a member name to a member path.
fn extend_path(path: &mut String, key: &str) {
    if !path.is_empty() {
        path.push('.');
    }
    path.push_str(key);
}

/// Places a member path relative to the listed labels.
fn reach<'a>(labels: &'a [CollectionName], path: &str) -> PathReach<'a> {
    let label = labels.iter().find(|label| label.as_str() == path);
    let through = labels
        .iter()
        .any(|label| label.as_str().strip_prefix(path).is_some_and(|rest| rest.starts_with('.')));

    match (label, through) {
        (None, false) => PathReach::Off,
        (None, true) => PathReach::Through,
        (Some(label), false) => PathReach::Label(label),
        (Some(label), true) => PathReach::LabelAndThrough(label),
    }
}

/// Whether an array is a collection: non-empty, with every item an object.
fn is_collection(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(Value::is_object)
}

/// Copies the object at `path` without the collections nested in it at any depth, and without the
/// objects on a listed path, since the walk turns each of those into a record or an enclosing item.
fn without_walked(labels: &[CollectionName], path: &str, map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .filter_map(|(key, value)| match value {
            Value::Array(items) if is_collection(items) => None,
            Value::Object(inner) => {
                let mut inner_path = path.to_owned();
                extend_path(&mut inner_path, key);
                match reach(labels, &inner_path) {
                    PathReach::Through | PathReach::Label(_) | PathReach::LabelAndThrough(_) => None,
                    PathReach::Off => Some((key.clone(), Value::Object(without_walked(labels, &inner_path, inner)))),
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Some((key.clone(), value.clone())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::json::{collection_split::split, error::JsonIngestError};
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_ir::record::Record;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn records(document: Value, labels: &[&str]) -> Result<Vec<Record>, JsonIngestError> {
        let labels: Vec<CollectionName> = labels.iter().copied().map(CollectionName::from).collect();
        let mut records = Vec::new();
        split(document, &labels, |record| {
            records.push(record);
            Ok::<(), JsonIngestError>(())
        })?;
        Ok(records)
    }

    fn data(record: &Record) -> Value {
        Value::Object(record.data().clone())
    }

    fn monitor() -> Value {
        json!({
            "data": {
                "monitors": [
                    {
                        "locationStop": { "properties": { "title": "Schwedenplatz", "attributes": { "rbl": 4116 } } },
                        "lines": [
                            {
                                "name": "U1",
                                "towards": "Oberlaa",
                                "departures": {
                                    "departure": [
                                        { "departureTime": { "countdown": 5 } },
                                        { "departureTime": { "countdown": 9 } }
                                    ]
                                }
                            },
                            {
                                "name": "U4",
                                "towards": "Hütteldorf",
                                "departures": { "departure": [{ "departureTime": { "countdown": 2 } }] }
                            }
                        ]
                    }
                ]
            },
            "message": { "value": "OK" }
        })
    }

    #[test]
    fn a_collection_three_arrays_down_yields_one_record_per_item_with_its_label() {
        let records = records(monitor(), &["data.monitors.lines.departures.departure"]).unwrap();

        assert_eq!(records.len(), 3);
        assert!(
            records
                .iter()
                .all(|record| record.collection() == &Some(CollectionName::from("data.monitors.lines.departures.departure")))
        );
    }

    #[test]
    fn a_record_carries_its_enclosing_items_under_the_names_of_their_arrays() {
        let records = records(monitor(), &["data.monitors.lines.departures.departure"]).unwrap();

        assert_eq!(
            data(&records[0]),
            json!({
                "departureTime": { "countdown": 5 },
                "lines": { "name": "U1", "towards": "Oberlaa" },
                "monitors": { "locationStop": { "properties": { "title": "Schwedenplatz", "attributes": { "rbl": 4116 } } } }
            })
        );
        assert_eq!(records[2].data().get("lines").and_then(|lines| lines.get("name")), Some(&json!("U4")));
    }

    #[test]
    fn an_enclosing_item_comes_without_its_nested_collections() {
        let records = records(monitor(), &["data.monitors.lines"]).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data().get("monitors").and_then(|monitors| monitors.get("lines")), None);
    }

    #[test]
    fn a_record_keeps_its_own_nested_collections() {
        let records = records(monitor(), &["data.monitors.lines"]).unwrap();

        assert_eq!(
            records[0].data().get("departures"),
            Some(&json!({ "departure": [{ "departureTime": { "countdown": 5 } }, { "departureTime": { "countdown": 9 } }] }))
        );
    }

    #[test]
    fn a_collection_that_is_not_listed_yields_no_records() {
        let records = records(monitor(), &["data.monitors.lines"]).unwrap();

        assert!(
            records
                .iter()
                .all(|record| record.collection() == &Some(CollectionName::from("data.monitors.lines")))
        );
    }

    #[test]
    fn a_listed_collection_and_one_nested_in_it_both_yield_records_in_document_order() {
        let records = records(monitor(), &["data.monitors.lines", "data.monitors.lines.departures.departure"]).unwrap();
        let labels: Vec<&str> = records
            .iter()
            .filter_map(|record| record.collection().as_ref())
            .map(CollectionName::as_str)
            .collect();

        assert_eq!(
            labels,
            vec![
                "data.monitors.lines",
                "data.monitors.lines.departures.departure",
                "data.monitors.lines.departures.departure",
                "data.monitors.lines",
                "data.monitors.lines.departures.departure",
            ]
        );
        assert_eq!(records[0].data().get("name"), Some(&json!("U1")));
        assert!(records[0].data().contains_key("departures"));
    }

    #[test]
    fn a_record_field_wins_over_an_enclosing_item_of_the_same_name() {
        let document = json!({ "lines": [{ "name": "U1", "stops": [{ "lines": "own" }] }] });
        let records = records(document, &["lines.stops"]).unwrap();

        assert_eq!(records[0].data().get("lines"), Some(&json!("own")));
    }

    #[test]
    fn the_nearest_enclosing_item_wins_over_a_farther_one_of_the_same_name() {
        let document = json!({ "items": [{ "level": "outer", "sub": { "items": [{ "level": "inner", "leaves": [{ "id": 1 }] }] } }] });
        let records = records(document, &["items.sub.items.leaves"]).unwrap();

        assert_eq!(records[0].data().get("items"), Some(&json!({ "level": "inner" })));
    }

    #[test]
    fn the_items_of_a_root_array_are_walked_without_becoming_enclosing_items() {
        let document = json!([{ "id": 1, "readings": [{ "t": 1 }] }, { "id": 2, "readings": [{ "t": 2 }] }]);
        let records = records(document, &["readings"]).unwrap();

        assert_eq!(records.iter().map(data).collect::<Vec<_>>(), vec![json!({ "t": 1 }), json!({ "t": 2 })]);
    }

    #[test]
    fn an_array_that_holds_anything_but_objects_is_no_collection() {
        let document = json!({ "mixed": [{ "a": 1 }, 5], "scalars": [1, 2] });

        assert!(records(document, &["mixed", "scalars"]).unwrap().is_empty());
    }

    #[test]
    fn an_empty_array_stays_a_field_of_an_enclosing_item() {
        let document = json!({ "lines": [{ "tags": [], "stops": [{ "id": 1 }] }] });
        let records = records(document, &["lines.stops"]).unwrap();

        assert_eq!(records[0].data().get("lines"), Some(&json!({ "tags": [] })));
    }

    #[test]
    fn a_label_that_matches_no_collection_yields_no_records() {
        assert!(records(monitor(), &["data.nowhere"]).unwrap().is_empty());
    }

    #[test]
    fn a_label_is_matched_on_whole_member_names_only() {
        assert!(records(monitor(), &["data.monitors.line"]).unwrap().is_empty());
    }

    /// Air-quality readings shaped like JSON converted from XML: an element that occurs once is an
    /// object, and one that occurs more often is an array.
    fn readings() -> Value {
        json!({
            "Index": {
                "@Group": "London",
                "Authority": [
                    {
                        "@Name": "Barking",
                        "Site": [
                            { "@Code": "BG1", "Species": [{ "@Code": "NO2", "@Index": "1" }, { "@Code": "PM10", "@Index": "2" }] },
                            { "@Code": "BG2", "Species": { "@Code": "NO2", "@Index": "3" } }
                        ]
                    },
                    {
                        "@Name": "Hackney",
                        "Site": { "@Code": "HK6", "Species": [{ "@Code": "NO2", "@Index": "4" }] }
                    }
                ]
            }
        })
    }

    #[test]
    fn a_lone_object_at_a_listed_path_is_a_collection_of_one_item() {
        let records = records(readings(), &["Index.Authority.Site.Species"]).unwrap();
        let indices: Vec<&Value> = records.iter().filter_map(|record| record.data().get("@Index")).collect();

        assert_eq!(indices, vec![&json!("1"), &json!("2"), &json!("3"), &json!("4")]);
    }

    #[test]
    fn a_lone_object_on_a_listed_path_encloses_the_records_below_it() {
        let records = records(readings(), &["Index.Authority.Site.Species"]).unwrap();

        assert_eq!(
            data(&records[3]),
            json!({
                "@Code": "NO2",
                "@Index": "4",
                "Site": { "@Code": "HK6" },
                "Authority": { "@Name": "Hackney" },
                "Index": { "@Group": "London" }
            })
        );
    }

    #[test]
    fn a_lone_site_and_an_array_of_sites_yield_the_same_record_shape() {
        let records = records(readings(), &["Index.Authority.Site"]).unwrap();

        assert_eq!(
            records.iter().map(|record| record.data().get("@Code")).collect::<Vec<_>>(),
            vec![Some(&json!("BG1")), Some(&json!("BG2")), Some(&json!("HK6"))]
        );
        assert_eq!(records[2].data().get("Authority"), Some(&json!({ "@Name": "Hackney" })));
    }

    #[test]
    fn an_object_on_a_listed_path_encloses_its_fields_without_the_members_on_the_path() {
        let document = json!({ "data": { "updated": 7, "lines": [{ "name": "U1", "stops": [{ "n": 1 }] }] } });
        let records = records(document, &["data.lines.stops"]).unwrap();

        assert_eq!(data(&records[0]), json!({ "n": 1, "lines": { "name": "U1" }, "data": { "updated": 7 } }));
    }

    #[test]
    fn an_enclosing_object_left_with_no_fields_is_not_added() {
        let document = json!({ "data": { "lines": [{ "stops": [{ "n": 1 }] }] } });
        let records = records(document, &["data.lines.stops"]).unwrap();

        assert_eq!(data(&records[0]), json!({ "n": 1 }));
    }

    #[test]
    fn a_lone_object_that_is_no_listed_path_stays_a_field_of_its_enclosing_item() {
        let document = json!({ "lines": [{ "name": "U1", "operator": { "id": "WL" }, "stops": [{ "n": 1 }] }] });
        let records = records(document, &["lines.stops"]).unwrap();

        assert_eq!(records[0].data().get("lines"), Some(&json!({ "name": "U1", "operator": { "id": "WL" } })));
    }

    #[test]
    fn a_scalar_root_is_rejected() {
        assert!(matches!(records(json!(5), &["a"]), Err(JsonIngestError::ExpectedObjectOrArray)));
        assert!(matches!(records(json!(null), &["a"]), Err(JsonIngestError::ExpectedObjectOrArray)));
    }

    #[test]
    fn an_error_from_emit_stops_the_walk() {
        let labels = [CollectionName::from("data.monitors.lines")];
        let mut seen = 0;
        let result = split(monitor(), &labels, |_| {
            seen += 1;
            Err(JsonIngestError::ExpectedObject)
        });

        assert!(matches!(result, Err(JsonIngestError::ExpectedObject)));
        assert_eq!(seen, 1);
    }
}

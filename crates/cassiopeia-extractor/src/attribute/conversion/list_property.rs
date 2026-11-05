use crate::{
    attribute::{
        conversion::{
            readers::{self, SourceParts},
            value_admission,
            value_reading::{self, ReadValue},
        },
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_mapping::conversion::{
    list_conversion::ListConversion,
    value_reading::{IdentifierReading, ValueReading},
};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// Reads the parts as the list a `ListProperty` holds, for a member of an object nested `mappings`
/// assemble: the items kept as read stay raw, null items included, for the attribute holding the
/// object to admit, and items read one way are the JSON the typed list serializes to.
///
/// # Errors
/// Returns what [`apply`] refuses for items read one way.
pub(crate) fn read(
    parts: SourceParts,
    conversion: ListConversion,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<JsonValue>, AttributeRefusal> {
    match conversion {
        ListConversion::AsRead => Ok(readers::aggregate(parts).map(JsonValue::Array)),
        ListConversion::Each(_) => Ok(apply(parts, conversion, policy, omitted)?.map(JsonValue::from)),
    }
}

/// Converts the parts into a `ListProperty`'s `valueList`, an ordered array of NGSI-LD Values (ETSI
/// GS CIM 009 v1.9.1 clause 4.5.21.2), in which no item can be null (clause 4.5.0).
///
/// The parts are gathered as the `array` transformation gathers them (see [`readers::aggregate`]),
/// so a source that is no list is its one item. Kept as read, the list is admitted as any value is,
/// without its null items (see [`value_admission::admit`]). Read one way, each item is read as that
/// reading reads one value: a null item is left out and counted in `omitted`, and an item the
/// reading refuses, or reads as nothing, refuses the whole list, since a list's order carries
/// meaning and no item may vanish from it unannounced.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] for a list of only nulls, and, for items read one
/// way, the refusal of the first item that could not be read.
pub(crate) fn apply(
    parts: SourceParts,
    conversion: ListConversion,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<Value>, AttributeRefusal> {
    let Some(items) = readers::aggregate(parts) else {
        return Ok(None);
    };

    match conversion {
        ListConversion::AsRead => value_admission::admit(JsonValue::Array(items), omitted),
        ListConversion::Each(reading) => read_each(items, reading, policy, omitted).map(Some),
    }
}

/// Reads every item of a list the way `reading` names, leaving out and counting its null items.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting a list of only nulls as not
/// [`RequiredValue::NonNull`], or the refusal of the first item that could not be read.
fn read_each(items: Vec<JsonValue>, reading: ValueReading, policy: Option<&GeometryPolicy>, omitted: &mut OmittedNulls) -> Result<Value, AttributeRefusal> {
    if items.iter().all(JsonValue::is_null) {
        return Err(AttributeRefusal::unusable(RequiredValue::NonNull, &JsonValue::Array(items).to_string()));
    }

    let mut list = Vec::with_capacity(items.len());
    for item in items {
        if item.is_null() {
            omitted.array_items += 1;
            continue;
        }
        list.push(read_item(item, reading, policy, omitted)?);
    }

    Ok(Value::Array(list))
}

/// Reads one item that is not null the way `reading` names, refusing one that reads as nothing.
///
/// Blank text is a value the record carries in a list, since the list was there, so every reading
/// but text, which keeps blank text as it is, refuses it. What the `object` and `json` readings keep
/// as raw JSON is admitted as any value is; an empty object or array admits nothing and is refused.
///
/// # Errors
/// Returns the refusal of the reading, or of an item that reads as nothing (see [`absent_item`]).
fn read_item(item: JsonValue, reading: ValueReading, policy: Option<&GeometryPolicy>, omitted: &mut OmittedNulls) -> Result<Value, AttributeRefusal> {
    if !matches!(reading, ValueReading::Text)
        && let Some(blank) = item.as_str().filter(|text| text.trim().is_empty())
    {
        return Err(absent_item(reading, blank));
    }

    match value_reading::read_value(item, reading, policy, omitted)? {
        Some(ReadValue::Value(value)) => Ok(value),
        Some(ReadValue::Json(json)) => {
            let emptied = match &json {
                JsonValue::Object(members) => members.is_empty().then_some("{}"),
                JsonValue::Array(items) => items.is_empty().then_some("[]"),
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => None,
            };
            value_admission::admit(json, omitted)?.ok_or_else(|| AttributeRefusal::unusable(RequiredValue::NonNull, emptied.unwrap_or("null")))
        }
        // Past the blank check, only JSON text spelling `null` reads as nothing.
        None => Err(absent_item(reading, "null")),
    }
}

/// The refusal of an item that is there but reads as nothing, quoting `text`, under the
/// requirement of the reading.
///
/// The text reading reads every item, so an item it found nothing in can only have held nothing
/// but null.
fn absent_item(reading: ValueReading, text: &str) -> AttributeRefusal {
    let required = match reading {
        ValueReading::Temporal(_) => return AttributeRefusal::UnreadableTimestamp { text: Box::from(text) },
        ValueReading::Boolean => RequiredValue::Boolean,
        ValueReading::Integer => RequiredValue::Integer,
        ValueReading::Float => RequiredValue::Float,
        ValueReading::Object => RequiredValue::JsonObject,
        ValueReading::Json => RequiredValue::Json,
        ValueReading::Geometry(_) => RequiredValue::Geometry,
        ValueReading::Identifier(IdentifierReading::Iri) => RequiredValue::Iri,
        ValueReading::Identifier(IdentifierReading::Urn) => RequiredValue::Urn,
        ValueReading::Text => RequiredValue::NonNull,
    };

    AttributeRefusal::unusable(required, text)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, converted, declared, default_of, omitted},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_mapping::{conversion::Conversion, transformation::Transformation};
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::{
            omitted_nulls::OmittedNulls,
            types::{Number, Value},
        },
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use smallvec::smallvec;

    #[test]
    fn an_array_part_under_the_list_property_default_becomes_the_list_itself() {
        let value = converted(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::ListProperty), None);

        assert_eq!(value, Some(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn a_scalar_part_under_the_list_property_default_becomes_a_one_element_list() {
        let value = converted(smallvec![json!("BS")], default_of(NgsiLdAttributeKind::ListProperty), None);

        assert_eq!(value, Some(Value::try_from(json!(["BS"])).unwrap()));
    }

    #[test]
    fn a_blank_or_null_part_under_the_list_property_default_is_no_value() {
        for part in [json!(""), json!("  "), json!(null)] {
            let value = converted(smallvec![part], default_of(NgsiLdAttributeKind::ListProperty), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn a_null_item_of_an_array_part_is_left_out_of_the_list_and_counted() {
        let (value, omitted) = omitted(smallvec![json!(["a", null, "b"])], default_of(NgsiLdAttributeKind::ListProperty));

        assert_eq!(value, Some(Value::try_from(json!(["a", "b"])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn array_text_under_the_list_property_default_becomes_the_list_it_encodes() {
        let value = converted(smallvec![json!("[1,7,15]")], default_of(NgsiLdAttributeKind::ListProperty), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 7, 15])).unwrap()));
    }

    #[test]
    fn a_null_item_of_array_text_is_left_out_of_the_list_and_counted() {
        let (value, omitted) = omitted(smallvec![json!(r#"["a",null,"b"]"#)], default_of(NgsiLdAttributeKind::ListProperty));

        assert_eq!(value, Some(Value::try_from(json!(["a", "b"])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn a_list_of_only_null_items_is_refused() {
        assert_eq!(
            apply(smallvec![json!([null])], default_of(NgsiLdAttributeKind::ListProperty), None),
            Err(AttributeRefusal::unusable(RequiredValue::NonNull, "[null]"))
        );
    }

    #[test]
    fn a_list_kept_as_read_leaves_out_its_null_items() {
        let (value, omitted) = omitted(smallvec![json!([1, "a", null])], default_of(NgsiLdAttributeKind::ListProperty));

        assert_eq!(value, Some(Value::try_from(json!([1, "a"])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    /// The conversion a `ListProperty` read from a source under `transformation` loads with.
    fn list(transformation: Transformation) -> Conversion {
        declared(NgsiLdAttributeKind::ListProperty, transformation)
    }

    /// The integers `values`, as a list of NGSI-LD Values.
    fn integers(values: &[i64]) -> Value {
        Value::Array(values.iter().map(|value| Value::Number(Number::Integer(*value))).collect())
    }

    #[test]
    fn integer_on_a_list_reads_each_item() {
        assert_eq!(
            converted(smallvec![json!(["1", "2", "3"])], list(Transformation::Integer), None),
            Some(integers(&[1, 2, 3]))
        );
    }

    #[test]
    fn an_unreadable_item_refuses_the_whole_list() {
        assert_eq!(
            apply(smallvec![json!(["1", "x"])], list(Transformation::Integer), None),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, "x"))
        );
    }

    #[test]
    fn a_null_item_is_left_out_of_a_typed_list() {
        let (value, omitted) = omitted(smallvec![json!(["1", null, "3"])], list(Transformation::Integer));

        assert_eq!(value, Some(integers(&[1, 3])));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn a_blank_item_refuses_a_typed_list() {
        assert_eq!(
            apply(smallvec![json!(["1", "", "3"])], list(Transformation::Integer), None),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, ""))
        );
    }

    #[test]
    fn string_on_a_list_writes_each_item_as_text() {
        assert_eq!(
            converted(smallvec![json!([1, true])], list(Transformation::String), None),
            Some(Value::try_from(json!(["1", "true"])).unwrap())
        );
    }

    #[test]
    fn datetime_on_a_list_reads_each_item_and_refuses_an_unreadable_one() {
        let value = converted(smallvec![json!(["2026-03-01T11:04:35Z"])], list(Transformation::DateTime), None);

        assert!(
            matches!(value, Some(Value::Array(ref items)) if matches!(items.as_slice(), [Value::Temporal(_)])),
            "{value:?}"
        );
        assert_eq!(
            apply(smallvec![json!(["nope"])], list(Transformation::DateTime), None),
            Err(AttributeRefusal::UnreadableTimestamp { text: Box::from("nope") })
        );
    }

    #[test]
    fn object_on_a_list_reads_each_item_as_an_object() {
        assert_eq!(
            converted(smallvec![json!([r#"{"a":1}"#, {"b": 2}])], list(Transformation::Object), None),
            Some(Value::try_from(json!([{"a": 1}, {"b": 2}])).unwrap())
        );
        assert_eq!(
            apply(smallvec![json!([1])], list(Transformation::Object), None),
            Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "1"))
        );
    }

    #[test]
    fn an_item_of_only_nulls_refuses_a_typed_list() {
        assert_eq!(
            apply(smallvec![json!([{"a": null}])], list(Transformation::Object), None),
            Err(AttributeRefusal::unusable(RequiredValue::NonNull, r#"{"a":null}"#))
        );
    }

    #[test]
    fn an_empty_item_refuses_a_typed_list() {
        assert_eq!(
            apply(smallvec![json!([{}])], list(Transformation::Object), None),
            Err(AttributeRefusal::unusable(RequiredValue::NonNull, "{}"))
        );
    }

    #[test]
    fn point_on_a_list_builds_a_point_per_item() {
        assert_eq!(
            converted(smallvec![json!([[1.0, 2.0], [3.0, 4.0]])], list(Transformation::Point), None),
            Some(Value::Array(vec![
                Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                    coordinates: [1.0, 2.0].into(),
                })),
                Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                    coordinates: [3.0, 4.0].into(),
                })),
            ]))
        );
    }

    #[test]
    fn a_lone_value_on_a_typed_list_is_its_one_item() {
        assert_eq!(converted(smallvec![json!("5")], list(Transformation::Integer), None), Some(integers(&[5])));
    }

    #[test]
    fn json_array_text_on_a_typed_list_is_read_item_by_item() {
        assert_eq!(
            converted(smallvec![json!(r#"["1","2"]"#)], list(Transformation::Integer), None),
            Some(integers(&[1, 2]))
        );
    }

    #[test]
    fn a_list_of_only_absent_parts_is_absent() {
        assert_eq!(apply(smallvec![json!(null), json!("")], list(Transformation::Integer), None), Ok(None));
    }

    #[test]
    fn a_typed_list_of_only_null_items_is_refused() {
        assert_eq!(
            apply(smallvec![json!([null, null])], list(Transformation::Integer), None),
            Err(AttributeRefusal::unusable(RequiredValue::NonNull, "[null,null]"))
        );
    }

    #[test]
    fn json_text_spelling_null_refuses_a_list_read_as_json() {
        assert_eq!(
            apply(smallvec![json!(["1", "null"])], list(Transformation::Json), None),
            Err(AttributeRefusal::unusable(RequiredValue::Json, "null"))
        );
    }

    #[test]
    fn geometry_on_a_list_keeps_a_geometry_collection_item() {
        let collection = json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]});

        assert_eq!(
            converted(smallvec![json!([collection])], list(Transformation::Geometry), None),
            Some(Value::Array(vec![Value::try_from(collection).unwrap()]))
        );
    }

    #[test]
    fn iri_on_a_list_normalizes_each_item() {
        assert_eq!(
            converted(smallvec![json!(["HTTP://A.org/x"])], list(Transformation::Iri), None),
            Some(Value::try_from(json!(["http://a.org/x"])).unwrap())
        );
        assert_eq!(
            apply(smallvec![json!(["urn:ex:b", "nope"])], list(Transformation::Urn), None),
            Err(AttributeRefusal::unusable(RequiredValue::Urn, "nope"))
        );
    }
}

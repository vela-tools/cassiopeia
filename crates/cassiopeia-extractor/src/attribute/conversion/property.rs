use crate::attribute::{
    conversion::{
        readers::{self, SourceParts},
        value_admission,
        value_reading::{self, ReadValue},
    },
    refusal::AttributeRefusal,
};
use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_mapping::conversion::property_conversion::PropertyConversion;
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// Reads the parts as a `Property`'s conversion names, before the value is admitted: `array` gathers
/// every part into one list, and every other conversion reads one value (see
/// [`value_reading::read`]).
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the reading could not use.
pub(crate) fn read(
    parts: SourceParts,
    conversion: PropertyConversion,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<ReadValue>, AttributeRefusal> {
    match conversion {
        PropertyConversion::One(reading) => value_reading::read(parts, reading, policy, omitted),
        PropertyConversion::Array => Ok(readers::aggregate(parts).map(|items| ReadValue::Json(JsonValue::Array(items)))),
    }
}

/// Converts the parts into a `Property`'s value, any NGSI-LD Value (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.2).
///
/// What the `object`, `json`, and `array` readings keep as raw JSON is admitted without its nulls
/// (see [`value_admission::admit`]); every other reading already built a value.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the reading could not use, or the value of only
/// nulls it read.
pub(crate) fn apply(
    parts: SourceParts,
    conversion: PropertyConversion,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<Value>, AttributeRefusal> {
    match read(parts, conversion, policy, omitted)? {
        Some(ReadValue::Json(json)) => value_admission::admit(json, omitted),
        Some(ReadValue::Value(value)) => Ok(Some(value)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::{
                self,
                fixtures::{applied_counting, apply, assembled, assembled_property, converted, default_of, omitted, property, street},
            },
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::{omitted_nulls::OmittedNulls, types::Value},
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use smallvec::smallvec;

    #[test]
    fn an_assembled_object_under_the_string_transformation_becomes_its_compact_json_text() {
        assert_eq!(
            apply(street(), assembled_property(Transformation::String), None),
            Ok(Some(AttributeContent::Value(Value::String(r#"{"street":"Ring"}"#.into()))))
        );
    }

    #[test]
    fn an_assembled_object_under_the_array_transformation_becomes_a_list_of_the_object() {
        assert_eq!(
            apply(street(), assembled_property(Transformation::Array), None),
            Ok(Some(AttributeContent::Value(Value::try_from(json!([{"street": "Ring"}])).unwrap())))
        );
    }

    #[test]
    fn an_assembled_object_under_the_object_and_json_transformations_stays_the_object() {
        for transformation in [Transformation::Object, Transformation::Json] {
            assert_eq!(
                apply(street(), assembled_property(transformation), None),
                Ok(Some(AttributeContent::Value(Value::try_from(json!({"street": "Ring"})).unwrap()))),
                "{transformation}"
            );
        }
    }

    #[test]
    fn an_assembled_object_spelling_no_geometry_under_a_geometry_transformation_is_refused_quoting_it() {
        for transformation in [Transformation::Geometry, Transformation::Point, Transformation::MultiPolygon] {
            assert_eq!(
                apply(street(), assembled_property(transformation), None),
                Err(AttributeRefusal::unusable(RequiredValue::Geometry, r#"{"street":"Ring"}"#)),
                "{transformation}"
            );
        }
    }

    #[test]
    fn several_string_parts_are_concatenated() {
        let value = converted(smallvec![json!("Station-"), json!(42)], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String("Station-42".into())));
    }

    #[test]
    fn string_parts_past_the_inline_capacity_are_concatenated_in_order() {
        // Four parts spill the inline capacity of one onto the heap; order must survive the spill.
        let value = converted(
            smallvec![json!("Station-"), json!(42), json!("/"), json!("north")],
            property(Transformation::String),
            None,
        );

        assert_eq!(value, Some(Value::String("Station-42/north".into())));
    }

    #[test]
    fn a_boolean_part_under_the_property_default_becomes_its_text() {
        let value = converted(smallvec![json!(true)], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String("true".into())));
    }

    #[test]
    fn an_array_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!(["BS", "IN"])], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_array_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_object_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!({"a": 1, "b": ["x"]})], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"{"a":1,"b":["x"]}"#.into())));
    }

    #[test]
    fn an_object_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!({"a": 1, "b": ["x"]})], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String(r#"{"a":1,"b":["x"]}"#.into())));
    }

    #[test]
    fn a_whole_float_part_under_the_property_default_becomes_its_shortest_text() {
        let value = converted(smallvec![json!(3.0)], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String("3".into())));
    }

    #[test]
    fn a_null_member_of_an_object_part_is_left_out_of_the_object_and_counted() {
        let (value, omitted) = omitted(smallvec![json!({"value": 21.5, "unit": null})], property(Transformation::Object));

        assert_eq!(value, Some(Value::try_from(json!({"value": 21.5})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
        );
    }

    #[test]
    fn a_null_part_is_an_absent_field_and_is_not_counted() {
        let (value, omitted) = omitted(smallvec![json!("A"), json!(null)], property(Transformation::Array));

        assert_eq!(value, Some(Value::try_from(json!(["A"])).unwrap()));
        assert!(omitted.is_empty());
    }

    #[test]
    fn an_empty_string_under_the_property_default_is_kept_as_a_value() {
        assert_eq!(
            converted(smallvec![json!("")], default_of(NgsiLdAttributeKind::Property), None),
            Some(Value::String("".into()))
        );
    }

    #[test]
    fn an_array_transformation_flattens_and_drops_nulls() {
        let value = converted(smallvec![json!([1, 2]), json!(null), json!(3)], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 2, 3])).unwrap()));
    }

    #[test]
    fn an_empty_array_transformation_is_null() {
        let value = converted(smallvec![json!(null)], property(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_array_transformation_drops_blank_and_null_parts() {
        let value = converted(smallvec![json!(""), json!(null), json!("A")], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!(["A"])).unwrap()));
    }

    #[test]
    fn an_array_transformation_over_only_blank_and_null_parts_is_null() {
        let value = converted(smallvec![json!(""), json!(null)], property(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_array_transformation_keeps_blank_elements_of_an_array_part() {
        let value = converted(smallvec![json!(["a", ""])], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!(["a", ""])).unwrap()));
    }

    #[test]
    fn an_object_transformation_keeps_an_object_part() {
        let value = converted(smallvec![json!({"a": 1})], property(Transformation::Object), None);

        assert_eq!(value, Some(Value::try_from(json!({"a": 1})).unwrap()));
    }

    #[test]
    fn an_object_transformation_reads_text_written_as_an_object() {
        let value = converted(smallvec![json!(r#"{"k":1}"#)], property(Transformation::Object), None);

        assert_eq!(value, Some(Value::try_from(json!({"k": 1})).unwrap()));
    }

    #[test]
    fn a_null_member_of_object_text_is_left_out_of_the_object_and_counted() {
        let (value, omitted) = omitted(smallvec![json!(r#"{"value":21.5,"unit":null}"#)], property(Transformation::Object));

        assert_eq!(value, Some(Value::try_from(json!({"value": 21.5})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
        );
    }

    #[test]
    fn text_that_is_not_json_under_an_object_transformation_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("{not json")], property(Transformation::Object), None),
            Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "{not json"))
        );
    }

    #[test]
    fn json_text_that_is_no_object_under_an_object_transformation_is_refused_quoting_it() {
        for text in ["[1,2]", "5", "\"k\"", "null"] {
            assert_eq!(
                apply(smallvec![json!(text)], property(Transformation::Object), None),
                Err(AttributeRefusal::unusable(RequiredValue::JsonObject, text)),
                "{text}"
            );
        }
    }

    #[test]
    fn blank_text_and_empty_object_text_under_an_object_transformation_are_no_value() {
        for text in ["", "  ", "{}"] {
            let value = converted(smallvec![json!(text)], property(Transformation::Object), None);

            assert_eq!(value, None, "{text:?}");
        }
    }

    #[test]
    fn a_structured_value_that_is_no_object_under_an_object_transformation_is_refused_quoting_its_rendering() {
        for (part, rendering) in [(json!([1, 2]), "[1,2]"), (json!(5), "5"), (json!(true), "true")] {
            assert_eq!(
                apply(smallvec![part], property(Transformation::Object), None),
                Err(AttributeRefusal::unusable(RequiredValue::JsonObject, rendering)),
                "{rendering}"
            );
        }
    }

    #[test]
    fn a_null_part_under_an_object_transformation_is_no_value_rather_than_a_refusal() {
        assert_eq!(converted(smallvec![json!(null)], property(Transformation::Object), None), None);
    }

    #[test]
    fn an_array_transformation_splices_in_text_written_as_an_array() {
        let value = converted(smallvec![json!("[1,2]")], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 2])).unwrap()));
    }

    #[test]
    fn text_that_is_no_json_array_under_an_array_transformation_is_one_element() {
        for text in ["BS IN", "[1,2", "5", "true", r#"{"k":1}"#] {
            let value = converted(smallvec![json!(text)], property(Transformation::Array), None);

            assert_eq!(value, Some(Value::try_from(json!([text])).unwrap()), "{text}");
        }
    }

    #[test]
    fn array_text_and_other_parts_are_flattened_in_order() {
        let value = converted(smallvec![json!("[1,2]"), json!("BS"), json!([3])], property(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!([1, 2, "BS", 3])).unwrap()));
    }

    #[test]
    fn empty_array_text_under_an_array_transformation_is_no_value() {
        let value = converted(smallvec![json!("[]")], property(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_json_transformation_reads_text_as_the_json_value_it_is_written_as() {
        for (text, expected) in [
            (r#"{"no2":3,"pm10":2}"#, json!({"no2": 3, "pm10": 2})),
            ("[1,7,15]", json!([1, 7, 15])),
            ("3.5", json!(3.5)),
            ("true", json!(true)),
            (r#""hello""#, json!("hello")),
        ] {
            let value = converted(smallvec![json!(text)], property(Transformation::Json), None);

            assert_eq!(value, Some(Value::try_from(expected).unwrap()), "{text}");
        }
    }

    #[test]
    fn a_value_that_is_not_text_under_a_json_transformation_is_kept_as_it_is() {
        for part in [json!(7), json!(false), json!([1, "a"]), json!({"k": 1})] {
            // The part is consumed by the conversion and still needed to build the expected value.
            let value = converted(smallvec![part.clone()], property(Transformation::Json), None);

            assert_eq!(value, Some(Value::try_from(part).unwrap()));
        }
    }

    #[test]
    fn text_that_is_not_json_under_a_json_transformation_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("hello")], property(Transformation::Json), None),
            Err(AttributeRefusal::unusable(RequiredValue::Json, "hello"))
        );
    }

    #[test]
    fn blank_text_and_null_text_under_a_json_transformation_are_no_value() {
        for part in [json!(""), json!("  "), json!("null"), json!(null)] {
            let value = converted(smallvec![part], property(Transformation::Json), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn a_null_nested_in_json_text_under_a_json_transformation_is_left_out_and_counted() {
        let (value, omitted) = omitted(smallvec![json!("[1,null]")], property(Transformation::Json));

        assert_eq!(value, Some(Value::try_from(json!([1])).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 0,
            }
        );
    }

    #[test]
    fn an_object_transformation_over_an_empty_object_is_null() {
        let value = converted(smallvec![json!({})], property(Transformation::Object), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_explicit_string_transformation_still_turns_a_geometry_into_its_compact_json_text() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = converted(smallvec![source], property(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"{"type":"Point","coordinates":[14.5,46.05]}"#.into())));
    }

    #[test]
    fn an_object_of_only_nulls_on_a_property_is_refused() {
        let (content, omitted) = applied_counting(smallvec![json!({"maybe": null})], property(Transformation::Object));

        assert_eq!(content, Err(AttributeRefusal::unusable(RequiredValue::NonNull, r#"{"maybe":null}"#)));
        assert!(omitted.is_empty());
    }

    #[test]
    fn an_array_of_only_nulls_on_a_property_is_refused() {
        assert_eq!(
            apply(smallvec![json!([null, null])], property(Transformation::Array), None),
            Err(AttributeRefusal::unusable(RequiredValue::NonNull, "[null,null]"))
        );
    }

    #[test]
    fn an_empty_array_or_object_on_a_property_is_absent() {
        assert_eq!(apply(smallvec![json!([])], property(Transformation::Array), None), Ok(None));
        assert_eq!(apply(smallvec![json!({})], property(Transformation::Object), None), Ok(None));
    }

    #[test]
    fn json_text_on_a_property_leaves_out_its_nulls() {
        let (value, omitted) = omitted(smallvec![json!(r#"{"a":null,"b":1}"#)], property(Transformation::Json));

        assert_eq!(value, Some(Value::try_from(json!({"b": 1})).unwrap()));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 0,
                object_members: 1,
            }
        );
    }

    #[test]
    fn string_writes_compact_json_text_and_keeps_an_empty_string() {
        assert_eq!(
            converted(smallvec![json!({"a": 1})], property(Transformation::String), None),
            Some(Value::String(r#"{"a":1}"#.into()))
        );
        assert_eq!(
            converted(smallvec![json!("")], property(Transformation::String), None),
            Some(Value::String("".into()))
        );
    }

    #[test]
    fn an_assembled_object_under_the_property_default_leaves_out_a_member_s_nulls_and_counts_them() {
        let (value, omitted) = omitted(smallvec![json!({"info": {"a": null, "b": 1}})], assembled(NgsiLdAttributeKind::Property, None));

        assert_eq!(value, Some(Value::try_from(json!({"info": {"b": 1}})).unwrap()));
        assert_eq!(omitted.object_members, 1);
    }

    #[test]
    fn an_assembled_point_under_the_multipoint_transformation_becomes_a_multipoint() {
        let point = smallvec![json!({"type": "Point", "coordinates": [16.37, 48.2]})];

        let expected: NgsiLdGeometry = serde_json::from_value(json!({"type": "MultiPoint", "coordinates": [[16.37, 48.2]]})).unwrap();
        assert_eq!(
            apply(point, assembled_property(Transformation::MultiPoint), None),
            Ok(Some(AttributeContent::Value(Value::Geospatial(Box::new(expected)))))
        );
    }

    #[test]
    fn the_member_read_of_an_object_keeps_its_nulls_for_the_holding_attribute_to_admit() {
        let mut omitted = OmittedNulls::default();

        assert_eq!(
            conversion::read_member(smallvec![json!({"a": null})], property(Transformation::Object), None, &mut omitted),
            Ok(Some(json!({"a": null})))
        );
        assert!(omitted.is_empty());
    }

    #[test]
    fn the_member_read_of_a_typed_value_is_the_json_it_serializes_to() {
        let at = conversion::read_member(
            smallvec![json!("2026-03-01 11:04:35")],
            property(Transformation::DateTime),
            None,
            &mut OmittedNulls::default(),
        );

        assert_eq!(at, Ok(Some(json!("2026-03-01T11:04:35Z"))));
    }
}

use crate::attribute::{
    conversion::{
        readers::{self, SourceParts},
        value_admission,
    },
    refusal::AttributeRefusal,
};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// Reads the parts as the list a `ListProperty` holds, before it is admitted: every part gathered as
/// the `array` transformation gathers them (see [`readers::aggregate`]), null items included.
pub(crate) fn read(parts: SourceParts) -> Option<JsonValue> {
    readers::aggregate(parts).map(JsonValue::Array)
}

/// Converts the parts into a `ListProperty`'s `valueList`, an ordered array of NGSI-LD Values (ETSI
/// GS CIM 009 v1.9.1 clause 4.5.21.2), in which no item can be null (clause 4.5.0).
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] for a list of only nulls (see
/// [`value_admission::admit`]).
pub(crate) fn apply(parts: SourceParts, omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
    read(parts).map_or(Ok(None), |list| value_admission::admit(list, omitted))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, converted, default_of, omitted},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::{omitted_nulls::OmittedNulls, types::Value},
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
}

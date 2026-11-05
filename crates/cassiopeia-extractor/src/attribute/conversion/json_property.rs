use crate::attribute::{
    conversion::{
        attribute_conversion::AttributeConversion,
        read_context::ReadContext,
        readers::{self, SourceParts},
    },
    refusal::AttributeRefusal,
};
use cassiopeia_mapping::conversion::json_conversion::JsonConversion;
use cassiopeia_ngsi_ld::value::{json_payload::JsonPayload, omitted_nulls::OmittedNulls};
use serde_json::Value as JsonValue;

/// A `JsonProperty`'s `json` member, raw JSON that NGSI-LD never interprets (ETSI GS CIM 009 v1.9.1
/// clause 4.5.24.2), so every null nested in it is kept as the source has it.
///
/// Naming no transformation keeps the parts as the source held them (see [`readers::keep_as_read`]);
/// `json` does so after reading each text part as the JSON it is written as; `object` keeps an
/// object, or reads one from its text, and keeps it even when empty; and `array` gathers every part
/// into one list. Reading refuses text that is not the JSON the conversion asks for, or, under
/// `object`, a value that is no object; admitting refuses what is not "a raw JSON object (or array of
/// objects)" (clause 4.5.24.2), such as an array holding a null item.
impl AttributeConversion for JsonConversion {
    type Content = JsonPayload;
    type Reading = JsonValue;

    fn read(self, parts: SourceParts, _ctx: &mut ReadContext<'_>) -> Result<Option<JsonValue>, AttributeRefusal> {
        match self {
            JsonConversion::Verbatim => Ok(readers::keep_as_read(parts)),
            JsonConversion::Json => Ok(readers::keep_as_read(readers::decode_parts(parts)?)),
            JsonConversion::Object => Ok(readers::merge_text(parts).map_or(Ok(None), readers::read_object)?.map(JsonValue::Object)),
            JsonConversion::Array => Ok(readers::aggregate(parts).map(JsonValue::Array)),
        }
    }

    fn admit(self, reading: JsonValue, _omitted: &mut OmittedNulls) -> Result<Option<JsonPayload>, AttributeRefusal> {
        Ok(Some(JsonPayload::try_from(reading)?))
    }

    fn into_json(reading: JsonValue) -> JsonValue {
        reading
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{applied_counting, apply, assembled, declared, default_of, member, payload, street},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_mapping::{conversion::Conversion, transformation::Transformation};
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::json_payload_error::{JsonPayloadError, JsonShape},
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use smallvec::smallvec;

    #[test]
    fn an_object_part_under_the_json_property_default_is_kept_as_it_is() {
        let payload = payload(smallvec![json!({"a": 1, "b": ["x"]})], default_of(NgsiLdAttributeKind::JsonProperty));

        assert_eq!(payload, Some(json!({"a": 1, "b": ["x"]})));
    }

    #[test]
    fn a_null_nested_in_a_json_property_part_is_kept() {
        let payload = payload(
            smallvec![json!({"maybe": null, "list": [1, null]})],
            default_of(NgsiLdAttributeKind::JsonProperty),
        );

        assert_eq!(payload, Some(json!({"maybe": null, "list": [1, null]})));
    }

    #[test]
    fn an_array_of_objects_under_the_json_property_default_is_kept_as_it_is() {
        let payload = payload(smallvec![json!([{"a": 1}, {"b": 2}])], default_of(NgsiLdAttributeKind::JsonProperty));

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn a_scalar_part_under_the_json_property_default_is_refused() {
        assert_eq!(
            apply(smallvec![json!(3.5)], default_of(NgsiLdAttributeKind::JsonProperty), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::Number }))
        );
    }

    #[test]
    fn a_null_part_under_the_json_property_default_is_no_value() {
        assert_eq!(payload(smallvec![json!(null)], default_of(NgsiLdAttributeKind::JsonProperty)), None);
    }

    #[test]
    fn several_parts_under_the_json_property_default_become_an_array_of_the_present_parts_in_order() {
        let payload = payload(
            smallvec![json!({"a": 1}), json!(null), json!({"b": null})],
            default_of(NgsiLdAttributeKind::JsonProperty),
        );

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": null}])));
    }

    #[test]
    fn several_parts_under_the_json_property_default_holding_a_non_object_are_refused() {
        assert_eq!(
            apply(smallvec![json!({"a": 1}), json!([2])], default_of(NgsiLdAttributeKind::JsonProperty), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::ItemNotAnObject { found: JsonShape::Array }))
        );
    }

    #[test]
    fn several_null_parts_under_the_json_property_default_are_no_value() {
        assert_eq!(
            payload(smallvec![json!(null), json!(null)], default_of(NgsiLdAttributeKind::JsonProperty)),
            None
        );
    }

    #[test]
    fn text_under_the_json_property_default_is_not_decoded() {
        assert_eq!(
            apply(smallvec![json!(r#"{"k":1}"#)], default_of(NgsiLdAttributeKind::JsonProperty), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::String }))
        );
    }

    #[test]
    fn a_decoded_conversion_reads_object_text_as_raw_json_keeping_its_nulls() {
        let payload = payload(smallvec![json!(r#"{"no2":3,"pm10":null}"#)], json_property(Transformation::Json));

        assert_eq!(payload, Some(json!({"no2": 3, "pm10": null})));
    }

    #[test]
    fn a_decoded_conversion_reads_text_of_an_array_of_objects() {
        let payload = payload(smallvec![json!(r#"[{"a":1},{"b":2}]"#)], json_property(Transformation::Json));

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn a_decoded_conversion_keeps_a_structured_part_as_it_is() {
        let payload = payload(smallvec![json!({"maybe": null})], json_property(Transformation::Json));

        assert_eq!(payload, Some(json!({"maybe": null})));
    }

    #[test]
    fn text_that_is_not_json_under_a_decoded_conversion_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("{not json")], json_property(Transformation::Json), None),
            Err(AttributeRefusal::unusable(RequiredValue::Json, "{not json"))
        );
    }

    #[test]
    fn scalar_text_under_a_decoded_conversion_is_no_json_property_payload() {
        assert_eq!(
            apply(smallvec![json!("5")], json_property(Transformation::Json), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::Number }))
        );
    }

    #[test]
    fn blank_text_under_a_decoded_conversion_is_no_value() {
        assert_eq!(payload(smallvec![json!("  ")], json_property(Transformation::Json)), None);
    }

    #[test]
    fn several_parts_under_a_decoded_conversion_become_an_array_of_the_present_decoded_parts() {
        let payload = payload(smallvec![json!(r#"{"a":1}"#), json!(""), json!({"b": 2})], json_property(Transformation::Json));

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    /// The conversion a `JsonProperty` read from a source under `transformation` loads with.
    fn json_property(transformation: Transformation) -> Conversion {
        declared(NgsiLdAttributeKind::JsonProperty, transformation)
    }

    #[test]
    fn an_assembled_object_kept_as_assembled_or_decoded_is_its_payload() {
        for transformation in [None, Some(Transformation::Json), Some(Transformation::Object)] {
            assert_eq!(
                payload(street(), assembled(NgsiLdAttributeKind::JsonProperty, transformation)),
                Some(json!({"street": "Ring"})),
                "{transformation:?}"
            );
        }
    }

    #[test]
    fn object_on_a_json_property_keeps_its_nulls() {
        assert_eq!(
            payload(smallvec![json!({"maybe": null})], json_property(Transformation::Object)),
            Some(json!({"maybe": null}))
        );
    }

    #[test]
    fn object_text_on_a_json_property_keeps_its_nulls() {
        assert_eq!(
            payload(smallvec![json!(r#"{"a":null}"#)], json_property(Transformation::Object)),
            Some(json!({"a": null}))
        );
    }

    #[test]
    fn object_on_a_json_property_refuses_a_value_that_is_no_object() {
        assert_eq!(
            apply(smallvec![json!([{"a": 1}])], json_property(Transformation::Object), None),
            Err(AttributeRefusal::unusable(RequiredValue::JsonObject, r#"[{"a":1}]"#))
        );
    }

    #[test]
    fn object_on_a_json_property_keeps_an_empty_object() {
        assert_eq!(payload(smallvec![json!({})], json_property(Transformation::Object)), Some(json!({})));
    }

    #[test]
    fn json_on_a_json_property_keeps_its_nulls_and_refuses_what_is_no_payload() {
        assert_eq!(
            payload(smallvec![json!(r#"[{"a":null}]"#)], json_property(Transformation::Json)),
            Some(json!([{"a": null}]))
        );
        assert_eq!(
            apply(smallvec![json!("12")], json_property(Transformation::Json), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::Number }))
        );
        assert_eq!(
            apply(smallvec![json!("nope")], json_property(Transformation::Json), None),
            Err(AttributeRefusal::unusable(RequiredValue::Json, "nope"))
        );
    }

    #[test]
    fn array_on_a_json_property_collects_its_parts_keeping_their_nulls() {
        assert_eq!(
            payload(smallvec![json!([{"a": null}]), json!({"b": 2})], json_property(Transformation::Array)),
            Some(json!([{"a": null}, {"b": 2}]))
        );
    }

    #[test]
    fn array_on_a_json_property_refuses_a_null_item() {
        assert_eq!(
            apply(smallvec![json!([{"a": 1}, null])], json_property(Transformation::Array), None),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::ItemNotAnObject { found: JsonShape::Null }))
        );
    }

    #[test]
    fn array_text_on_a_json_property_is_read() {
        assert_eq!(
            payload(smallvec![json!(r#"[{"a":1}]"#)], json_property(Transformation::Array)),
            Some(json!([{"a": 1}]))
        );
    }

    #[test]
    fn no_json_property_conversion_counts_a_dropped_null() {
        for (part, conversion) in [
            (json!({"maybe": null}), default_of(NgsiLdAttributeKind::JsonProperty)),
            (json!({"maybe": null}), json_property(Transformation::Object)),
            (json!(r#"{"a":null}"#), json_property(Transformation::Object)),
            (json!(r#"[{"a":null}]"#), json_property(Transformation::Json)),
            (json!([{"a": null}]), json_property(Transformation::Array)),
        ] {
            let (content, omitted) = applied_counting(smallvec![part], conversion);

            assert!(matches!(content, Ok(Some(_))), "{conversion:?}: {content:?}");
            assert!(omitted.is_empty(), "{conversion:?}");
        }
    }

    #[test]
    fn the_member_read_of_a_json_property_is_its_raw_json_even_when_no_payload() {
        assert_eq!(
            member(smallvec![json!(3.5)], default_of(NgsiLdAttributeKind::JsonProperty)).0,
            Ok(Some(json!(3.5)))
        );
    }
}

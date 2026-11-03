use crate::attribute::refusal::AttributeRefusal;
use cassiopeia_geometry::{error::GeometryError, policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_mapping::{transformation::Transformation, value_conversion::ValueConversion};
use cassiopeia_ngsi_ld::value::{
    json_payload::JsonPayload,
    omitted_nulls::OmittedNulls,
    parsing::parse_decimal,
    types::{TemporalValue, Value},
};
use compact_str::CompactString;
use serde_json::Value as JsonValue;
use smallvec::SmallVec;

/// Which temporal NGSI-LD value a transformation produces.
#[derive(Clone, Copy)]
enum TemporalKind {
    DateTime,
    Date,
    Time,
}

/// The raw source values read for one attribute declaration, one per source template.
///
/// A declaration names a single source in the overwhelmingly common case, so the inline capacity of
/// one keeps every leaf, language entry, instance, and metadata sub-attribute of every record off the
/// heap, where a `Vec` would cost one malloc/free pair per resolved declaration.
pub(crate) type SourceParts = SmallVec<[JsonValue; 1]>;

/// Converts the raw source values read for an attribute into its content.
pub(crate) struct Transformer;

impl Transformer {
    /// Applies a conversion to the source values collected for one attribute.
    ///
    /// The conversion is already resolved: the declared transformation, or the default the
    /// attribute's kind implies (see [`ValueConversion::default_for`]). A verbatim conversion keeps
    /// what the source held as raw JSON. Under a transformation, `Array` aggregates all parts; every
    /// other transformation first merges the parts into one intermediate value and then coerces it to
    /// the target NGSI-LD type.
    ///
    /// Reading a source into an NGSI-LD Value leaves out every null nested in it, since none can sit
    /// in a Value (ETSI GS CIM 009 v1.9.1 clause 4.5.0), and counts each in `omitted`. Raw JSON keeps
    /// its nulls.
    ///
    /// `Ok(None)` means the source carried no value. Three conversions can refuse instead: a
    /// transformation naming a geometry type routes through the geometry lattice, where the source
    /// may carry a geometry a `GeoProperty` cannot hold or one this mapping did not authorise
    /// converting; a temporal transformation refuses text that reads as no supported spelling of a
    /// date-time; and raw JSON that is no object or array of objects is no `JsonProperty` payload.
    /// Every other conversion drops an unusable value and cannot fail.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the attribute could not take.
    pub(crate) fn apply(
        parts: SourceParts,
        conversion: ValueConversion,
        geometry: Option<&GeometryPolicy>,
        omitted: &mut OmittedNulls,
    ) -> Result<Option<AttributeContent>, AttributeRefusal> {
        match conversion {
            ValueConversion::Verbatim => Ok(Self::keep_as_read(parts)?.map(AttributeContent::Json)),
            ValueConversion::Transform(transformation) => {
                let value = match transformation.geometry_target() {
                    Some(target) => match Self::merge_structured(parts, omitted) {
                        Some(merged) => Self::coerce_geometry(&merged, target, geometry)?,
                        None => None,
                    },
                    None => Self::coerce_value(parts, transformation, omitted)?,
                };
                Ok(value.map(AttributeContent::Value))
            }
        }
    }

    /// Keeps the parts as the source held them, as raw JSON: a lone part is its own payload, several
    /// are an array of the parts in order.
    ///
    /// Nothing is coerced, stringified, or filtered by JSON type, because the payload is raw JSON
    /// that NGSI-LD never interprets (ETSI GS CIM 009 v1.9.1 clause 4.5.24.2), so a null nested in a
    /// part is kept. A part that is null is a field the record does not carry and is left out, and
    /// when no part is left there is no payload.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::JsonPayload`] when what is left is not "a raw JSON object (or array
    /// of objects)" (clause 4.5.24.2).
    fn keep_as_read(parts: SourceParts) -> Result<Option<JsonPayload>, AttributeRefusal> {
        let mut parts = parts.into_iter().filter(|part| !part.is_null()).collect::<SourceParts>();
        let json = match parts.len() {
            0 => return Ok(None),
            1 => parts.swap_remove(0),
            _ => JsonValue::Array(parts.into_vec()),
        };

        Ok(Some(JsonPayload::try_from(json)?))
    }

    /// Applies a transformation that names no geometry type.
    ///
    /// The geometry transformations are routed away before this point and name no scalar or container
    /// value of their own, so they resolve to no value here.
    fn coerce_value(parts: SourceParts, transformation: Transformation, omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
        match transformation {
            Transformation::Array => Ok(Self::aggregate(parts, omitted)),
            Transformation::Boolean => Ok(Self::merge_first(parts, omitted).as_ref().and_then(Value::to_boolean).map(Value::from)),
            Transformation::Integer => Ok(Self::merge_first(parts, omitted).as_ref().and_then(Self::coerce_integer)),
            Transformation::Float => Ok(Self::merge_first(parts, omitted).as_ref().and_then(Self::coerce_float)),
            Transformation::String => Ok(Self::merge_text(parts, omitted).map(Self::coerce_string)),
            Transformation::Object => Ok(Self::merge_text(parts, omitted).and_then(Self::coerce_object)),
            Transformation::DateTime => Self::coerce_temporal(Self::merge_text(parts, omitted), TemporalKind::DateTime),
            Transformation::Date => Self::coerce_temporal(Self::merge_text(parts, omitted), TemporalKind::Date),
            Transformation::Time => Self::coerce_temporal(Self::merge_text(parts, omitted), TemporalKind::Time),
            Transformation::Geometry
            | Transformation::Point
            | Transformation::MultiPoint
            | Transformation::LineString
            | Transformation::MultiLineString
            | Transformation::Polygon
            | Transformation::MultiPolygon => Ok(None),
        }
    }

    /// Flattens the parts into one array, dropping absent parts and splicing in any nested arrays.
    ///
    /// A part is absent when it is null or blank text: CSV and spreadsheet sources spell an empty
    /// field as an empty string, so a blank is a value the record does not carry, and keeping it would
    /// emit `[""]` for an empty cell. Only whole parts are judged that way; the elements of an array
    /// part are the source's own data and are spliced in as they are, except that a null element is
    /// left out and counted in `omitted`, since a `valueList` is an ordered array of NGSI-LD Values
    /// (ETSI GS CIM 009 v1.9.1 clause 4.5.21.2) and null is none (clause 3.1).
    fn aggregate(parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        let mut result = Vec::new();
        for part in parts {
            match part {
                JsonValue::Array(array) => result.extend(array),
                JsonValue::Null => {}
                JsonValue::String(text) if text.trim().is_empty() => {}
                JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Object(_) => result.push(part),
            }
        }

        match Value::from_json(JsonValue::Array(result), omitted) {
            Some(Value::Array(items)) if items.is_empty() => None,
            aggregated => aggregated,
        }
    }

    /// Merges parts for a scalar target: a lone part keeps its type, several take the first non-null.
    fn merge_first(mut parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        if parts.len() == 1 {
            return Value::from_json(parts.swap_remove(0), omitted);
        }

        let first = parts.into_iter().find(|part| !part.is_null())?;
        Value::from_json(first, omitted)
    }

    /// Merges parts for a text target: a lone part keeps its type, several are concatenated.
    fn merge_text(mut parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        if parts.len() == 1 {
            return Value::from_json(parts.swap_remove(0), omitted);
        }

        let combined = parts
            .iter()
            .filter(|part| !part.is_null())
            .map(|part| match part {
                JsonValue::String(text) => text.clone(),
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => part.to_string(),
            })
            .collect::<String>();

        if combined.is_empty() {
            None
        } else {
            Some(Value::String(CompactString::from(combined)))
        }
    }

    /// Merges parts for a geometry target: a lone part keeps its type, several keep their array
    /// structure so their order (the coordinate order) survives.
    ///
    /// A part that is null is a field the record does not carry, not a null inside the source's own
    /// data, so it is left out without being counted.
    fn merge_structured(mut parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
        if parts.len() == 1 {
            return Value::from_json(parts.swap_remove(0), omitted);
        }

        let present = parts.into_iter().filter(|part| !part.is_null()).collect::<Vec<JsonValue>>();
        if present.is_empty() {
            return None;
        }
        Value::from_json(JsonValue::Array(present), omitted)
    }

    /// Coerces the merged value to an integer, parsing numeric strings explicitly.
    ///
    /// A float-shaped string truncates toward zero through the same conversion the value model uses,
    /// so a value outside the `i64` range is no integer rather than saturating.
    fn coerce_integer(value: &Value) -> Option<Value> {
        match value {
            Value::String(text) => text
                .parse::<i64>()
                .ok()
                .or_else(|| text.parse::<f64>().ok().and_then(|float| Value::from(float).to_integer()))
                .map(Value::from),
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_integer().map(Value::from)
            }
        }
    }

    /// Coerces the merged value to a float, accepting a comma as a decimal separator.
    fn coerce_float(value: &Value) -> Option<Value> {
        match value {
            Value::String(text) => parse_decimal(text.as_str()).map(Value::from),
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_float().map(Value::from)
            }
        }
    }

    /// Coerces the merged value to a string.
    fn coerce_string(value: Value) -> Value {
        match &value {
            Value::String(_) => value,
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                Value::String(CompactString::from(value.to_string()))
            }
        }
    }

    /// Keeps the merged value when it is a non-empty JSON object, and yields no value for anything
    /// else.
    fn coerce_object(value: Value) -> Option<Value> {
        match value {
            Value::Object(object) if !object.is_empty() => Some(Value::Object(object)),
            Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }

    /// Reads the merged value as the geometry the transformation names, under the mapping's policy.
    ///
    /// A value that carries no geometry at all yields no value, so the attribute is simply omitted,
    /// the way an absent source field always has been. A value that carries a geometry which cannot
    /// legally become the declared type is refused instead, so the run can say what it dropped and
    /// why. The resolver also calls this directly for a geometry it assembled from nested `mappings`,
    /// which has no source parts left to merge.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] naming why the geometry cannot become the declared type.
    pub(crate) fn coerce_geometry(value: &Value, target: GeometryTarget, policy: Option<&GeometryPolicy>) -> Result<Option<Value>, GeometryError> {
        let policy = policy.copied().unwrap_or_default();
        let geometry = value.to_geometry(target, &policy)?;

        Ok(geometry.map(|geometry| Value::Geospatial(Box::new(geometry))))
    }

    /// Coerces the merged value to a temporal value of the given kind.
    ///
    /// An absent source resolves to no value, and so does a blank one: CSV and spreadsheet sources
    /// spell an empty field as an empty string, so a blank is a value the record does not carry
    /// rather than one it carries wrongly, and naming every empty cell of a column would drown the
    /// run. Any other value that is there and will not read is refused instead, text and non-text
    /// alike (a number too small to be an epoch, a boolean, an array), because a timestamp that
    /// vanishes unannounced is what leaves a published entity with nothing anchoring it in time.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnreadableTimestamp`] quoting the value that would not read: text
    /// as written, any other value as its rendering.
    fn coerce_temporal(value: Option<Value>, kind: TemporalKind) -> Result<Option<Value>, AttributeRefusal> {
        let Some(value) = value else {
            return Ok(None);
        };

        if let Some(datetime) = value.try_parse_datetime() {
            let temporal = match kind {
                TemporalKind::DateTime => TemporalValue::DateTime(datetime),
                TemporalKind::Date => TemporalValue::Date(datetime),
                TemporalKind::Time => TemporalValue::Time(datetime),
            };
            return Ok(Some(Value::Temporal(temporal)));
        }

        match value.as_str().map(str::trim) {
            Some("") => Ok(None),
            Some(text) => Err(AttributeRefusal::UnreadableTimestamp { text: Box::from(text) }),
            None => Err(AttributeRefusal::UnreadableTimestamp {
                text: value.to_string().into_boxed_str(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::{
        refusal::AttributeRefusal,
        transformer::{SourceParts, Transformer},
    };
    use cassiopeia_geometry::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
    };
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_mapping::{transformation::Transformation, value_conversion::ValueConversion};
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::{
            json_payload_error::{JsonPayloadError, JsonShape},
            omitted_nulls::OmittedNulls,
            types::{Number, Value},
        },
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    /// Applies a conversion with a fresh null count, which these tests do not inspect.
    fn apply(parts: SourceParts, conversion: ValueConversion, policy: Option<&GeometryPolicy>) -> Result<Option<AttributeContent>, AttributeRefusal> {
        Transformer::apply(parts, conversion, policy, &mut OmittedNulls::default())
    }

    /// Applies a conversion expected to succeed with an NGSI-LD Value or with no value.
    fn converted(parts: SourceParts, conversion: ValueConversion, policy: Option<&GeometryPolicy>) -> Option<Value> {
        match apply(parts, conversion, policy).unwrap() {
            Some(AttributeContent::Value(value)) => Some(value),
            Some(AttributeContent::Json(payload)) => panic!("raw JSON {payload:?}, not a value"),
            None => None,
        }
    }

    /// Applies a conversion expected to succeed with raw JSON or with no value.
    fn payload(parts: SourceParts, conversion: ValueConversion) -> Option<JsonValue> {
        match apply(parts, conversion, None).unwrap() {
            Some(AttributeContent::Json(payload)) => Some(JsonValue::from(payload)),
            Some(AttributeContent::Value(value)) => panic!("a value {value:?}, not raw JSON"),
            None => None,
        }
    }

    /// Applies a conversion and returns the nulls it left out of the value.
    fn omitted(parts: SourceParts, conversion: ValueConversion) -> (Option<Value>, OmittedNulls) {
        let mut omitted = OmittedNulls::default();
        let content = Transformer::apply(parts, conversion, None, &mut omitted).unwrap();
        let value = match content {
            Some(AttributeContent::Value(value)) => Some(value),
            Some(AttributeContent::Json(payload)) => panic!("raw JSON {payload:?}, not a value"),
            None => None,
        };
        (value, omitted)
    }

    #[test]
    fn a_lone_numeric_part_keeps_its_type_under_a_float_transformation() {
        let value = converted(smallvec![json!(25.5)], ValueConversion::Transform(Transformation::Float), None);

        assert_eq!(value, Some(Value::Number(Number::Float(25.5))));
    }

    #[test]
    fn a_numeric_string_is_parsed_to_an_integer() {
        let value = converted(smallvec![json!("7")], ValueConversion::Transform(Transformation::Integer), None);

        assert_eq!(value, Some(Value::Number(Number::Integer(7))));
    }

    #[test]
    fn a_float_string_is_truncated_to_an_integer() {
        let value = converted(smallvec![json!("7.9")], ValueConversion::Transform(Transformation::Integer), None);

        assert_eq!(value, Some(Value::Number(Number::Integer(7))));
    }

    #[test]
    fn a_comma_decimal_string_parses_as_a_float() {
        let value = converted(smallvec![json!("1,5")], ValueConversion::Transform(Transformation::Float), None);

        assert_eq!(value, Some(Value::Number(Number::Float(1.5))));
    }

    #[test]
    fn several_string_parts_are_concatenated() {
        let value = converted(
            smallvec![json!("Station-"), json!(42)],
            ValueConversion::Transform(Transformation::String),
            None,
        );

        assert_eq!(value, Some(Value::String("Station-42".into())));
    }

    #[test]
    fn string_parts_past_the_inline_capacity_are_concatenated_in_order() {
        // Four parts spill the inline capacity of one onto the heap; order must survive the spill.
        let value = converted(
            smallvec![json!("Station-"), json!(42), json!("/"), json!("north")],
            ValueConversion::Transform(Transformation::String),
            None,
        );

        assert_eq!(value, Some(Value::String("Station-42/north".into())));
    }

    /// The conversion an attribute of `kind` declared without a transformation resolves to.
    const fn default_of(kind: NgsiLdAttributeKind) -> ValueConversion {
        ValueConversion::default_for(kind)
    }

    #[test]
    fn a_boolean_part_under_the_property_default_becomes_its_text() {
        let value = converted(smallvec![json!(true)], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String("true".into())));
    }

    #[test]
    fn an_array_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!(["BS", "IN"])], ValueConversion::Transform(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_array_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::Property), None);

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_object_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = converted(smallvec![json!({"a": 1, "b": ["x"]})], ValueConversion::Transform(Transformation::String), None);

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
    fn a_null_member_of_an_object_part_is_left_out_of_the_object_and_counted() {
        let (value, omitted) = omitted(
            smallvec![json!({"value": 21.5, "unit": null})],
            ValueConversion::Transform(Transformation::Object),
        );

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
        let (value, omitted) = omitted(smallvec![json!("A"), json!(null)], ValueConversion::Transform(Transformation::Array));

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
    fn text_spelling_no_finite_number_under_a_float_transformation_is_no_value() {
        assert_eq!(
            converted(smallvec![json!("NaN")], ValueConversion::Transform(Transformation::Float), None),
            None
        );
    }

    #[test]
    fn an_array_transformation_flattens_and_drops_nulls() {
        let value = converted(
            smallvec![json!([1, 2]), json!(null), json!(3)],
            ValueConversion::Transform(Transformation::Array),
            None,
        );

        assert_eq!(value, Some(Value::try_from(json!([1, 2, 3])).unwrap()));
    }

    #[test]
    fn an_empty_array_transformation_is_null() {
        let value = converted(smallvec![json!(null)], ValueConversion::Transform(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_array_transformation_drops_blank_and_null_parts() {
        let value = converted(
            smallvec![json!(""), json!(null), json!("A")],
            ValueConversion::Transform(Transformation::Array),
            None,
        );

        assert_eq!(value, Some(Value::try_from(json!(["A"])).unwrap()));
    }

    #[test]
    fn an_array_transformation_over_only_blank_and_null_parts_is_null() {
        let value = converted(smallvec![json!(""), json!(null)], ValueConversion::Transform(Transformation::Array), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_array_transformation_keeps_blank_elements_of_an_array_part() {
        let value = converted(smallvec![json!(["a", ""])], ValueConversion::Transform(Transformation::Array), None);

        assert_eq!(value, Some(Value::try_from(json!(["a", ""])).unwrap()));
    }

    #[test]
    fn an_object_transformation_keeps_an_object_part() {
        let value = converted(smallvec![json!({"a": 1})], ValueConversion::Transform(Transformation::Object), None);

        assert_eq!(value, Some(Value::try_from(json!({"a": 1})).unwrap()));
    }

    #[test]
    fn an_object_transformation_does_not_parse_text() {
        let value = converted(smallvec![json!(r#"{"a":1}"#)], ValueConversion::Transform(Transformation::Object), None);

        assert_eq!(value, None);
    }

    #[test]
    fn an_object_transformation_over_an_empty_object_is_null() {
        let value = converted(smallvec![json!({})], ValueConversion::Transform(Transformation::Object), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_numeric_epoch_reads_as_the_same_instant_as_its_text() {
        for transformation in [Transformation::DateTime, Transformation::Date, Transformation::Time] {
            let number = converted(smallvec![json!(1_775_253_620)], ValueConversion::Transform(transformation), None);
            let text = converted(smallvec![json!("1775253620")], ValueConversion::Transform(transformation), None);

            assert!(matches!(number, Some(Value::Temporal(_))), "{transformation:?} left {number:?}");
            assert_eq!(number, text);
        }
    }

    #[test]
    fn a_fractional_numeric_epoch_reads_as_an_instant() {
        let value = converted(smallvec![json!(1_775_253_620.5)], ValueConversion::Transform(Transformation::DateTime), None);

        assert!(matches!(value, Some(Value::Temporal(_))));
    }

    #[test]
    fn a_number_too_small_for_an_epoch_is_refused_quoting_it() {
        let refusal = apply(smallvec![json!(2026)], ValueConversion::Transform(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "2026"));
    }

    #[test]
    fn a_boolean_under_a_temporal_transformation_is_refused_quoting_it() {
        let refusal = apply(smallvec![json!(true)], ValueConversion::Transform(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "true"));
    }

    #[test]
    fn a_blank_temporal_source_is_still_absent_rather_than_refused() {
        let value = converted(smallvec![json!("  ")], ValueConversion::Transform(Transformation::DateTime), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_point_transformation_builds_a_geospatial_value() {
        let value = converted(smallvec![json!(14.5), json!(46.0)], ValueConversion::Transform(Transformation::Point), None);

        assert!(matches!(value, Some(Value::Geospatial(_))));
    }

    #[test]
    fn a_geometry_transformation_keeps_an_already_formed_geometry() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = converted(smallvec![source], ValueConversion::Transform(Transformation::Geometry), None);

        assert!(matches!(value, Some(Value::Geospatial(_))));
    }

    #[test]
    fn a_geometry_transformation_on_a_non_geometry_value_is_null() {
        let value = converted(smallvec![json!("not a geometry")], ValueConversion::Transform(Transformation::Geometry), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_geojson_object_under_the_geo_property_default_is_kept_as_its_geometry() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = converted(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None);

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [14.5, 46.05].into(),
            })))
        );
    }

    #[test]
    fn geojson_text_under_the_geo_property_default_is_read_as_its_geometry() {
        let source = json!(r#"{"type":"Point","coordinates":[14.5,46.05]}"#);
        let value = converted(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None);

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [14.5, 46.05].into(),
            })))
        );
    }

    #[test]
    fn a_value_carrying_no_geometry_under_the_geo_property_default_is_no_value_rather_than_a_refusal() {
        for part in [json!("not a geometry"), json!(""), json!("  "), json!(null)] {
            let value = converted(smallvec![part], default_of(NgsiLdAttributeKind::GeoProperty), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn the_geo_property_default_applies_the_declared_geometry_policy() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]],
        });
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Centroid)).build();
        let value = converted(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), Some(&policy));

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [1.0, 1.0].into(),
            })))
        );
    }

    #[test]
    fn a_source_geometry_collection_under_the_geo_property_default_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            apply(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn an_explicit_string_transformation_still_turns_a_geometry_into_its_compact_json_text() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = converted(smallvec![source], ValueConversion::Transform(Transformation::String), None);

        assert_eq!(value, Some(Value::String(r#"{"type":"Point","coordinates":[14.5,46.05]}"#.into())));
    }

    #[test]
    fn a_point_source_promotes_to_a_declared_multipoint() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = converted(smallvec![source], ValueConversion::Transform(Transformation::MultiPoint), None);

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::MultiPoint);
    }

    #[test]
    fn a_lossy_geometry_conversion_is_refused_rather_than_silently_dropped() {
        let source = json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]],
            ],
        });

        assert_eq!(
            apply(smallvec![source], ValueConversion::Transform(Transformation::Polygon), None),
            Err(AttributeRefusal::Geometry(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            }))
        );
    }

    #[test]
    fn a_declared_conversion_makes_the_same_source_convert() {
        let source = json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [8.0, 5.0], [8.0, 8.0], [5.0, 5.0]]],
            ],
        });
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Largest)).build();
        let value = converted(smallvec![source], ValueConversion::Transform(Transformation::Polygon), Some(&policy));

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::Polygon);
    }

    #[test]
    fn a_source_geometry_collection_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            apply(smallvec![source], ValueConversion::Transform(Transformation::Geometry), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn a_clockwise_exterior_ring_is_emitted_counterclockwise() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]],
        });
        let value = converted(smallvec![source], ValueConversion::Transform(Transformation::Polygon), None);

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(
            *geometry,
            NgsiLdGeometry::Polygon {
                coordinates: vec![vec![
                    [0.0, 0.0].into(),
                    [1.0, 0.0].into(),
                    [1.0, 1.0].into(),
                    [0.0, 1.0].into(),
                    [0.0, 0.0].into(),
                ]],
            }
        );
    }

    #[test]
    fn a_null_part_under_a_scalar_transformation_stays_null() {
        let value = converted(smallvec![json!(null)], ValueConversion::Transform(Transformation::Integer), None);

        assert_eq!(value, None);
    }
}

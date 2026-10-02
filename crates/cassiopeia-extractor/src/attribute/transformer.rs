use crate::attribute::refusal::AttributeRefusal;
use cassiopeia_geometry::{error::GeometryError, policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_mapping::{transformation::Transformation, value_conversion::ValueConversion};
use cassiopeia_ngsi_ld::value::{
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

/// Converts the raw source values read for an attribute into a single NGSI-LD value.
pub(crate) struct Transformer;

impl Transformer {
    /// Applies a conversion to the source values collected for one attribute.
    ///
    /// The conversion is already resolved: the declared transformation, or the default the
    /// attribute's kind implies (see [`ValueConversion::default_for`]). A verbatim conversion keeps
    /// what the source held. Under a transformation, `Array` aggregates all parts; every other
    /// transformation first merges the parts into one intermediate value and then coerces it to the
    /// target NGSI-LD type.
    ///
    /// Two transformations can refuse. A transformation naming a geometry type routes through the
    /// geometry lattice, where the source may carry a geometry a `GeoProperty` cannot hold or one
    /// this mapping did not authorise converting; a temporal transformation refuses text that reads
    /// as no supported spelling of a date-time. Every other conversion drops an unusable value to
    /// null and cannot fail.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the attribute could not take.
    pub(crate) fn apply(parts: SourceParts, conversion: ValueConversion, geometry: Option<&GeometryPolicy>) -> Result<Value, AttributeRefusal> {
        match conversion {
            ValueConversion::Verbatim => Ok(Self::keep_as_read(parts)),
            ValueConversion::Transform(transformation) => match transformation.geometry_target() {
                Some(target) => Ok(Self::coerce_geometry(&Self::merge_structured(parts), target, geometry)?),
                None => Self::coerce_value(parts, transformation),
            },
        }
    }

    /// Keeps the parts as the source held them: a lone part is its own value, several are an array
    /// of the parts in order, and parts that are all null are no value.
    ///
    /// Nothing is coerced, stringified, or filtered by JSON type, because the value is raw JSON that
    /// NGSI-LD never interprets (ETSI GS CIM 009 v1.9.1 clause 4.5.24.2, Table 5.2.38-1).
    fn keep_as_read(parts: SourceParts) -> Value {
        if parts.iter().all(JsonValue::is_null) {
            return Value::Null;
        }

        Self::merge_structured(parts)
    }

    /// Applies a transformation that names no geometry type.
    ///
    /// The geometry transformations are routed away before this point and name no scalar or container
    /// value of their own, so they resolve to null here.
    fn coerce_value(parts: SourceParts, transformation: Transformation) -> Result<Value, AttributeRefusal> {
        match transformation {
            Transformation::Array => Ok(Self::aggregate(parts)),
            Transformation::Boolean => Ok(Self::coerce_boolean(&Self::merge_first(parts))),
            Transformation::Integer => Ok(Self::coerce_integer(&Self::merge_first(parts))),
            Transformation::Float => Ok(Self::coerce_float(&Self::merge_first(parts))),
            Transformation::String => Ok(Self::coerce_string(Self::merge_text(parts))),
            Transformation::Object => Ok(Self::coerce_object(Self::merge_text(parts))),
            Transformation::DateTime => Self::coerce_temporal(&Self::merge_text(parts), TemporalKind::DateTime),
            Transformation::Date => Self::coerce_temporal(&Self::merge_text(parts), TemporalKind::Date),
            Transformation::Time => Self::coerce_temporal(&Self::merge_text(parts), TemporalKind::Time),
            Transformation::Geometry
            | Transformation::Point
            | Transformation::MultiPoint
            | Transformation::LineString
            | Transformation::MultiLineString
            | Transformation::Polygon
            | Transformation::MultiPolygon => Ok(Value::Null),
        }
    }

    /// Flattens the parts into one array, dropping absent parts and splicing in any nested arrays.
    ///
    /// A part is absent when it is null or blank text: CSV and spreadsheet sources spell an empty
    /// field as an empty string, so a blank is a value the record does not carry, and keeping it would
    /// emit `[""]` for an empty cell. Only whole parts are judged; the elements of an array part are
    /// the source's own data and are spliced in as they are.
    fn aggregate(parts: SourceParts) -> Value {
        let mut result = Vec::new();
        for part in parts {
            match part {
                JsonValue::Array(array) => result.extend(array),
                JsonValue::Null => {}
                JsonValue::String(text) if text.trim().is_empty() => {}
                JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Object(_) => result.push(part),
            }
        }

        if result.is_empty() {
            Value::Null
        } else {
            Value::from(JsonValue::Array(result))
        }
    }

    /// Merges parts for a scalar target: a lone part keeps its type, several take the first non-null.
    fn merge_first(mut parts: SourceParts) -> Value {
        if parts.len() == 1 {
            return Value::from(parts.swap_remove(0));
        }

        let first = parts.into_iter().find(|part| !part.is_null()).unwrap_or(JsonValue::Null);
        Value::from(first)
    }

    /// Merges parts for a text target: a lone part keeps its type, several are concatenated.
    fn merge_text(mut parts: SourceParts) -> Value {
        if parts.len() == 1 {
            return Value::from(parts.swap_remove(0));
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
            Value::Null
        } else {
            Value::String(CompactString::from(combined))
        }
    }

    /// Merges parts for a structured target, a geometry or a verbatim value: a lone part keeps its
    /// type, several keep their array structure so their order (for a geometry, the coordinate order)
    /// survives.
    fn merge_structured(mut parts: SourceParts) -> Value {
        if parts.len() == 1 {
            return Value::from(parts.swap_remove(0));
        }

        Value::from(JsonValue::Array(parts.into_vec()))
    }

    /// Coerces the merged value to a boolean.
    fn coerce_boolean(value: &Value) -> Value {
        match value {
            Value::Null => Value::Null,
            Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_boolean().map_or(Value::Null, Value::from)
            }
        }
    }

    /// Coerces the merged value to an integer, parsing numeric strings explicitly.
    ///
    /// A float-shaped string truncates toward zero through the same conversion the value model uses,
    /// so a value outside the `i64` range drops to null rather than saturating.
    fn coerce_integer(value: &Value) -> Value {
        match value {
            Value::Null => Value::Null,
            Value::String(text) => {
                if let Ok(integer) = text.parse::<i64>() {
                    Value::from(integer)
                } else if let Ok(float) = text.parse::<f64>() {
                    Value::from(float).to_integer().map_or(Value::Null, Value::from)
                } else {
                    Value::Null
                }
            }
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_integer().map_or(Value::Null, Value::from)
            }
        }
    }

    /// Coerces the merged value to a float, accepting a comma as a decimal separator.
    fn coerce_float(value: &Value) -> Value {
        match value {
            Value::Null => Value::Null,
            Value::String(text) => parse_decimal(text.as_str()).map_or(Value::Null, Value::from),
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                value.to_float().map_or(Value::Null, Value::from)
            }
        }
    }

    /// Coerces the merged value to a string, preserving nulls.
    fn coerce_string(value: Value) -> Value {
        match &value {
            Value::String(_) => value,
            Value::Null => Value::Null,
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                Value::String(CompactString::from(value.to_string()))
            }
        }
    }

    /// Keeps the merged value when it is a non-empty JSON object, and yields null for anything else.
    fn coerce_object(value: Value) -> Value {
        match value {
            Value::Object(object) if !object.is_empty() => Value::Object(object),
            Value::Null
            | Value::Boolean(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Temporal(_)
            | Value::Geospatial(_)
            | Value::Array(_)
            | Value::Object(_) => Value::Null,
        }
    }

    /// Reads the merged value as the geometry the transformation names, under the mapping's policy.
    ///
    /// A value that carries no geometry at all yields a null, so the attribute is simply omitted, the
    /// way an absent source field always has been. A value that carries a geometry which cannot
    /// legally become the declared type is refused instead, so the run can say what it dropped and
    /// why. The resolver also calls this directly for a geometry it assembled from nested `mappings`,
    /// which has no source parts left to merge.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] naming why the geometry cannot become the declared type.
    pub(crate) fn coerce_geometry(value: &Value, target: GeometryTarget, policy: Option<&GeometryPolicy>) -> Result<Value, GeometryError> {
        let policy = policy.copied().unwrap_or_default();
        let geometry = value.to_geometry(target, &policy)?;

        Ok(geometry.map_or(Value::Null, |geometry| Value::Geospatial(Box::new(geometry))))
    }

    /// Coerces the merged value to a temporal value of the given kind.
    ///
    /// An absent source resolves to null, and so does a blank one: CSV and spreadsheet sources spell
    /// an empty field as an empty string, so a blank is a value the record does not carry rather
    /// than one it carries wrongly, and naming every empty cell of a column would drown the run.
    /// Any other value that is there and will not read is refused instead, text and non-text alike
    /// (a number too small to be an epoch, a boolean, an array), because a timestamp that vanishes
    /// unannounced is what leaves a published entity with nothing anchoring it in time.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnreadableTimestamp`] quoting the value that would not read: text
    /// as written, any other value as its rendering.
    fn coerce_temporal(value: &Value, kind: TemporalKind) -> Result<Value, AttributeRefusal> {
        if value.is_null() {
            return Ok(Value::Null);
        }

        if let Some(datetime) = value.try_parse_datetime() {
            let temporal = match kind {
                TemporalKind::DateTime => TemporalValue::DateTime(datetime),
                TemporalKind::Date => TemporalValue::Date(datetime),
                TemporalKind::Time => TemporalValue::Time(datetime),
            };
            return Ok(Value::Temporal(temporal));
        }

        match value.as_str().map(str::trim) {
            Some("") => Ok(Value::Null),
            Some(text) => Err(AttributeRefusal::UnreadableTimestamp { text: Box::from(text) }),
            None => Err(AttributeRefusal::UnreadableTimestamp {
                text: value.to_string().into_boxed_str(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::{refusal::AttributeRefusal, transformer::Transformer};
    use cassiopeia_geometry::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
    };
    use cassiopeia_mapping::{transformation::Transformation, value_conversion::ValueConversion};
    use cassiopeia_ngsi_ld::{
        entity::attribute::NgsiLdAttributeKind,
        value::types::{Number, Value},
    };
    use serde_json::json;
    use smallvec::smallvec;

    #[test]
    fn a_lone_numeric_part_keeps_its_type_under_a_float_transformation() {
        let value = Transformer::apply(smallvec![json!(25.5)], ValueConversion::Transform(Transformation::Float), None).unwrap();

        assert_eq!(value, Value::Number(Number::Float(25.5)));
    }

    #[test]
    fn a_numeric_string_is_parsed_to_an_integer() {
        let value = Transformer::apply(smallvec![json!("7")], ValueConversion::Transform(Transformation::Integer), None).unwrap();

        assert_eq!(value, Value::Number(Number::Integer(7)));
    }

    #[test]
    fn a_float_string_is_truncated_to_an_integer() {
        let value = Transformer::apply(smallvec![json!("7.9")], ValueConversion::Transform(Transformation::Integer), None).unwrap();

        assert_eq!(value, Value::Number(Number::Integer(7)));
    }

    #[test]
    fn a_comma_decimal_string_parses_as_a_float() {
        let value = Transformer::apply(smallvec![json!("1,5")], ValueConversion::Transform(Transformation::Float), None).unwrap();

        assert_eq!(value, Value::Number(Number::Float(1.5)));
    }

    #[test]
    fn several_string_parts_are_concatenated() {
        let value = Transformer::apply(
            smallvec![json!("Station-"), json!(42)],
            ValueConversion::Transform(Transformation::String),
            None,
        )
        .unwrap();

        assert_eq!(value, Value::String("Station-42".into()));
    }

    #[test]
    fn string_parts_past_the_inline_capacity_are_concatenated_in_order() {
        // Four parts spill the inline capacity of one onto the heap; order must survive the spill.
        let value = Transformer::apply(
            smallvec![json!("Station-"), json!(42), json!("/"), json!("north")],
            ValueConversion::Transform(Transformation::String),
            None,
        )
        .unwrap();

        assert_eq!(value, Value::String("Station-42/north".into()));
    }

    /// The conversion an attribute of `kind` declared without a transformation resolves to.
    const fn default_of(kind: NgsiLdAttributeKind) -> ValueConversion {
        ValueConversion::default_for(kind)
    }

    #[test]
    fn a_boolean_part_under_the_property_default_becomes_its_text() {
        let value = Transformer::apply(smallvec![json!(true)], default_of(NgsiLdAttributeKind::Property), None).unwrap();

        assert_eq!(value, Value::String("true".into()));
    }

    #[test]
    fn an_array_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = Transformer::apply(smallvec![json!(["BS", "IN"])], ValueConversion::Transform(Transformation::String), None).unwrap();

        assert_eq!(value, Value::String(r#"["BS","IN"]"#.into()));
    }

    #[test]
    fn an_array_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = Transformer::apply(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::Property), None).unwrap();

        assert_eq!(value, Value::String(r#"["BS","IN"]"#.into()));
    }

    #[test]
    fn an_object_part_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = Transformer::apply(smallvec![json!({"a": 1, "b": ["x"]})], ValueConversion::Transform(Transformation::String), None).unwrap();

        assert_eq!(value, Value::String(r#"{"a":1,"b":["x"]}"#.into()));
    }

    #[test]
    fn an_object_part_under_the_property_default_becomes_its_compact_json_text() {
        let value = Transformer::apply(smallvec![json!({"a": 1, "b": ["x"]})], default_of(NgsiLdAttributeKind::Property), None).unwrap();

        assert_eq!(value, Value::String(r#"{"a":1,"b":["x"]}"#.into()));
    }

    #[test]
    fn a_whole_float_part_under_the_property_default_becomes_its_shortest_text() {
        let value = Transformer::apply(smallvec![json!(3.0)], default_of(NgsiLdAttributeKind::Property), None).unwrap();

        assert_eq!(value, Value::String("3".into()));
    }

    #[test]
    fn an_array_part_under_the_list_property_default_becomes_the_list_itself() {
        let value = Transformer::apply(smallvec![json!(["BS", "IN"])], default_of(NgsiLdAttributeKind::ListProperty), None).unwrap();

        assert_eq!(value, Value::from(json!(["BS", "IN"])));
    }

    #[test]
    fn a_scalar_part_under_the_list_property_default_becomes_a_one_element_list() {
        let value = Transformer::apply(smallvec![json!("BS")], default_of(NgsiLdAttributeKind::ListProperty), None).unwrap();

        assert_eq!(value, Value::from(json!(["BS"])));
    }

    #[test]
    fn a_blank_or_null_part_under_the_list_property_default_is_no_value() {
        for part in [json!(""), json!("  "), json!(null)] {
            let value = Transformer::apply(smallvec![part], default_of(NgsiLdAttributeKind::ListProperty), None).unwrap();

            assert!(value.is_null(), "{value:?}");
        }
    }

    #[test]
    fn an_object_part_under_the_json_property_default_is_kept_as_it_is() {
        let value = Transformer::apply(smallvec![json!({"a": 1, "b": ["x"]})], default_of(NgsiLdAttributeKind::JsonProperty), None).unwrap();

        assert_eq!(value, Value::from(json!({"a": 1, "b": ["x"]})));
    }

    #[test]
    fn an_array_part_under_the_json_property_default_is_kept_as_it_is() {
        let value = Transformer::apply(smallvec![json!([{"a": 1}, {"b": 2}])], default_of(NgsiLdAttributeKind::JsonProperty), None).unwrap();

        assert_eq!(value, Value::from(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn a_scalar_part_under_the_json_property_default_keeps_its_json_type() {
        let value = Transformer::apply(smallvec![json!(3.5)], default_of(NgsiLdAttributeKind::JsonProperty), None).unwrap();

        assert_eq!(value, Value::Number(Number::Float(3.5)));
    }

    #[test]
    fn a_null_part_under_the_json_property_default_is_no_value() {
        let value = Transformer::apply(smallvec![json!(null)], default_of(NgsiLdAttributeKind::JsonProperty), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn several_parts_under_the_json_property_default_become_an_array_of_the_parts_in_order() {
        let value = Transformer::apply(
            smallvec![json!({"a": 1}), json!(null), json!([2])],
            default_of(NgsiLdAttributeKind::JsonProperty),
            None,
        )
        .unwrap();

        assert_eq!(value, Value::from(json!([{"a": 1}, null, [2]])));
    }

    #[test]
    fn several_null_parts_under_the_json_property_default_are_no_value() {
        let value = Transformer::apply(smallvec![json!(null), json!(null)], default_of(NgsiLdAttributeKind::JsonProperty), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn an_array_transformation_flattens_and_drops_nulls() {
        let value = Transformer::apply(
            smallvec![json!([1, 2]), json!(null), json!(3)],
            ValueConversion::Transform(Transformation::Array),
            None,
        )
        .unwrap();

        assert_eq!(value, Value::from(json!([1, 2, 3])));
    }

    #[test]
    fn an_empty_array_transformation_is_null() {
        let value = Transformer::apply(smallvec![json!(null)], ValueConversion::Transform(Transformation::Array), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn an_array_transformation_drops_blank_and_null_parts() {
        let value = Transformer::apply(
            smallvec![json!(""), json!(null), json!("A")],
            ValueConversion::Transform(Transformation::Array),
            None,
        )
        .unwrap();

        assert_eq!(value, Value::from(json!(["A"])));
    }

    #[test]
    fn an_array_transformation_over_only_blank_and_null_parts_is_null() {
        let value = Transformer::apply(smallvec![json!(""), json!(null)], ValueConversion::Transform(Transformation::Array), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn an_array_transformation_keeps_blank_elements_of_an_array_part() {
        let value = Transformer::apply(smallvec![json!(["a", ""])], ValueConversion::Transform(Transformation::Array), None).unwrap();

        assert_eq!(value, Value::from(json!(["a", ""])));
    }

    #[test]
    fn an_object_transformation_keeps_an_object_part() {
        let value = Transformer::apply(smallvec![json!({"a": 1})], ValueConversion::Transform(Transformation::Object), None).unwrap();

        assert_eq!(value, Value::from(json!({"a": 1})));
    }

    #[test]
    fn an_object_transformation_does_not_parse_text() {
        let value = Transformer::apply(smallvec![json!(r#"{"a":1}"#)], ValueConversion::Transform(Transformation::Object), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn an_object_transformation_over_an_empty_object_is_null() {
        let value = Transformer::apply(smallvec![json!({})], ValueConversion::Transform(Transformation::Object), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn a_numeric_epoch_reads_as_the_same_instant_as_its_text() {
        for transformation in [Transformation::DateTime, Transformation::Date, Transformation::Time] {
            let number = Transformer::apply(smallvec![json!(1_775_253_620)], ValueConversion::Transform(transformation), None).unwrap();
            let text = Transformer::apply(smallvec![json!("1775253620")], ValueConversion::Transform(transformation), None).unwrap();

            assert!(matches!(number, Value::Temporal(_)), "{transformation:?} left {number:?}");
            assert_eq!(number, text);
        }
    }

    #[test]
    fn a_fractional_numeric_epoch_reads_as_an_instant() {
        let value = Transformer::apply(smallvec![json!(1_775_253_620.5)], ValueConversion::Transform(Transformation::DateTime), None).unwrap();

        assert!(matches!(value, Value::Temporal(_)));
    }

    #[test]
    fn a_number_too_small_for_an_epoch_is_refused_quoting_it() {
        let refusal = Transformer::apply(smallvec![json!(2026)], ValueConversion::Transform(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "2026"));
    }

    #[test]
    fn a_boolean_under_a_temporal_transformation_is_refused_quoting_it() {
        let refusal = Transformer::apply(smallvec![json!(true)], ValueConversion::Transform(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "true"));
    }

    #[test]
    fn a_blank_temporal_source_is_still_absent_rather_than_refused() {
        let value = Transformer::apply(smallvec![json!("  ")], ValueConversion::Transform(Transformation::DateTime), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn a_point_transformation_builds_a_geospatial_value() {
        let value = Transformer::apply(smallvec![json!(14.5), json!(46.0)], ValueConversion::Transform(Transformation::Point), None).unwrap();

        assert!(matches!(value, Value::Geospatial(_)));
    }

    #[test]
    fn a_geometry_transformation_keeps_an_already_formed_geometry() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::Geometry), None).unwrap();

        assert!(matches!(value, Value::Geospatial(_)));
    }

    #[test]
    fn a_geometry_transformation_on_a_non_geometry_value_is_null() {
        let value = Transformer::apply(smallvec![json!("not a geometry")], ValueConversion::Transform(Transformation::Geometry), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn a_geojson_object_under_the_geo_property_default_is_kept_as_its_geometry() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = Transformer::apply(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None).unwrap();

        assert_eq!(
            value,
            Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [14.5, 46.05].into(),
            }))
        );
    }

    #[test]
    fn geojson_text_under_the_geo_property_default_is_read_as_its_geometry() {
        let source = json!(r#"{"type":"Point","coordinates":[14.5,46.05]}"#);
        let value = Transformer::apply(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None).unwrap();

        assert_eq!(
            value,
            Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [14.5, 46.05].into(),
            }))
        );
    }

    #[test]
    fn a_value_carrying_no_geometry_under_the_geo_property_default_is_no_value_rather_than_a_refusal() {
        for part in [json!("not a geometry"), json!(""), json!("  "), json!(null)] {
            let value = Transformer::apply(smallvec![part], default_of(NgsiLdAttributeKind::GeoProperty), None).unwrap();

            assert!(value.is_null(), "{value:?}");
        }
    }

    #[test]
    fn the_geo_property_default_applies_the_declared_geometry_policy() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]],
        });
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Centroid)).build();
        let value = Transformer::apply(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), Some(&policy)).unwrap();

        assert_eq!(
            value,
            Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [1.0, 1.0].into(),
            }))
        );
    }

    #[test]
    fn a_source_geometry_collection_under_the_geo_property_default_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            Transformer::apply(smallvec![source], default_of(NgsiLdAttributeKind::GeoProperty), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn an_explicit_string_transformation_still_turns_a_geometry_into_its_compact_json_text() {
        let source = json!({"type": "Point", "coordinates": [14.5, 46.05]});
        let value = Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::String), None).unwrap();

        assert_eq!(value, Value::String(r#"{"type":"Point","coordinates":[14.5,46.05]}"#.into()));
    }

    #[test]
    fn a_point_source_promotes_to_a_declared_multipoint() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::MultiPoint), None).unwrap();

        let Value::Geospatial(geometry) = value else {
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
            Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::Polygon), None),
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
        let value = Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::Polygon), Some(&policy)).unwrap();

        let Value::Geospatial(geometry) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::Polygon);
    }

    #[test]
    fn a_source_geometry_collection_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::Geometry), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn a_clockwise_exterior_ring_is_emitted_counterclockwise() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]],
        });
        let value = Transformer::apply(smallvec![source], ValueConversion::Transform(Transformation::Polygon), None).unwrap();

        let Value::Geospatial(geometry) = value else {
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
        let value = Transformer::apply(smallvec![json!(null)], ValueConversion::Transform(Transformation::Integer), None).unwrap();

        assert!(value.is_null());
    }
}

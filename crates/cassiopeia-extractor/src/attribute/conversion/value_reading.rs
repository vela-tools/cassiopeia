use crate::{
    attribute::{
        conversion::{
            identifier,
            read_context::ReadContext,
            readers::{self, SourceParts},
        },
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_geometry::{policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_mapping::conversion::value_reading::{TemporalReading, ValueReading};
use cassiopeia_ngsi_ld::value::{
    omitted_nulls::OmittedNulls,
    types::{TemporalValue, Value},
};
use compact_str::CompactString;
use serde_json::Value as JsonValue;

/// What one reading of a declaration's source produced, before the declaration's type admits it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ReadValue {
    /// Raw JSON the `object` or `json` reading kept as the source held it, every null included.
    Json(JsonValue),
    /// A value a scalar, text, temporal, or geometry reading built.
    Value(Value),
}

/// Writes a reading as the JSON it stands for inside a structured value, such as a member of an
/// object nested `mappings` assemble: raw JSON as it was kept, nulls included, and a value as the
/// JSON it serializes to.
// Manual because each variant unwraps into the JSON it holds or stands for, which no derive
// expresses.
impl From<ReadValue> for JsonValue {
    fn from(reading: ReadValue) -> JsonValue {
        match reading {
            ReadValue::Json(json) => json,
            ReadValue::Value(value) => JsonValue::from(value),
        }
    }
}

/// Reads the source parts as one value, the way `reading` names.
///
/// A geometry reading keeps the parts' array structure so their order, the coordinate order,
/// survives (see [`geometry_source`]). A scalar reading takes the first part present, and every other
/// reading merges the parts into text; the merged value is then read on its own.
///
/// `Ok(None)` means the source carried no value. A temporal reading refuses text that reads as no
/// supported spelling of a date-time; a boolean, integer, or float reading refuses a value that
/// spells no such scalar; the object and JSON readings refuse text that does not read as the JSON
/// they ask for, and the object reading any other value that is no object; and a geometry reading
/// refuses a value that spells no geometry, a geometry the declared type cannot hold, or one this
/// mapping did not authorise converting. The text reading uses any value and never refuses one.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the reading could not use.
pub(crate) fn read(parts: SourceParts, reading: ValueReading, ctx: &mut ReadContext<'_>) -> Result<Option<ReadValue>, AttributeRefusal> {
    let merged = match reading {
        ValueReading::Geometry(target) => {
            let Some(value) = geometry_source(parts, ctx.omitted) else {
                return Ok(None);
            };
            return Ok(any_geometry(&value, target, ctx.policy)?.map(ReadValue::Value));
        }
        ValueReading::Boolean | ValueReading::Integer | ValueReading::Float => readers::merge_first(parts),
        ValueReading::Text | ValueReading::Object | ValueReading::Json | ValueReading::Temporal(_) | ValueReading::Identifier(_) => readers::merge_text(parts),
    };

    merged.map_or(Ok(None), |value| read_value(value, reading, ctx))
}

/// Reads one value, already merged from the parts or taken as one item of a list, the way
/// `reading` names.
///
/// The object and JSON readings keep raw JSON with every null in it. Every other reading builds a
/// value, which holds no null (ETSI GS CIM 009 v1.9.1 clause 4.5.0): a null nested in what it reads
/// is left out and counted in the context. A value that is there and will not read is refused, and a
/// blank one is no value (see [`AttributeRefusal::unless_blank`]).
pub(crate) fn read_value(value: JsonValue, reading: ValueReading, ctx: &mut ReadContext<'_>) -> Result<Option<ReadValue>, AttributeRefusal> {
    match reading {
        ValueReading::Object => Ok(readers::read_object(value, RequiredValue::JsonObject)?.map(|object| ReadValue::Json(JsonValue::Object(object)))),
        ValueReading::Json => Ok(readers::decode(value)?.map(ReadValue::Json)),
        ValueReading::Boolean => typed(value, ctx.omitted, |value| coerce(&value, reading, |value| value.to_boolean().map(Value::from))),
        ValueReading::Integer => typed(value, ctx.omitted, |value| coerce(&value, reading, |value| value.to_integer().map(Value::from))),
        ValueReading::Float => typed(value, ctx.omitted, |value| {
            coerce(&value, reading, |value| value.to_float().and_then(|float| Value::try_from(float).ok()))
        }),
        ValueReading::Text => typed(value, ctx.omitted, |value| Ok(Some(coerce_string(value)))),
        ValueReading::Temporal(temporal) => typed(value, ctx.omitted, |value| {
            coerce(&value, reading, |value| {
                value.try_parse_datetime().map(|datetime| {
                    Value::Temporal(match temporal {
                        TemporalReading::DateTime => TemporalValue::DateTime(datetime),
                        TemporalReading::Date => TemporalValue::Date(datetime),
                        TemporalReading::Time => TemporalValue::Time(datetime),
                    })
                })
            })
        }),
        ValueReading::Identifier(identifier) => typed(value, ctx.omitted, |value| {
            Ok(identifier::read_identifier(&value, identifier)?.map(|iri| Value::String(CompactString::from(iri.as_str()))))
        }),
        ValueReading::Geometry(target) => {
            let policy = ctx.policy;
            typed(value, ctx.omitted, |value| any_geometry(&value, target, policy))
        }
    }
}

/// Builds a value from raw JSON, leaving out and counting every null nested in it, and reads it
/// through `coerce`.
fn typed(
    value: JsonValue,
    omitted: &mut OmittedNulls,
    coerce: impl FnOnce(Value) -> Result<Option<Value>, AttributeRefusal>,
) -> Result<Option<ReadValue>, AttributeRefusal> {
    Ok(Value::from_json(value, omitted).map_or(Ok(None), coerce)?.map(ReadValue::Value))
}

/// Reads the source parts as text: text as it is, and any other value as its compact JSON text.
///
/// The text reading never refuses a value. A null nested in a value written as text is left out of
/// the text and counted in `omitted`.
pub(crate) fn read_text(parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
    readers::merge_text(parts).and_then(|value| Value::from_json(value, omitted)).map(coerce_string)
}

/// Merges the source parts into the one value a geometry is read from, or `None` when the parts
/// carry no value.
///
/// The parts carry no value when every one of them is absent (see [`readers::is_absent`]), as an
/// empty cell, or a pair of empty coordinate cells, is. Any other parts are merged keeping their
/// array structure, so their order, the coordinate order, survives (see
/// [`readers::merge_structured`]). A null nested in the merged value is left out and counted in
/// `omitted`.
pub(crate) fn geometry_source(parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
    if parts.iter().all(readers::is_absent) {
        return None;
    }

    readers::merge_structured(parts).and_then(|merged| Value::from_json(merged, omitted))
}

/// Reads a value as `reading` reads it through `read`, refusing a value that is there and that
/// `read` finds nothing in (see [`AttributeRefusal::unless_blank`]).
///
/// # Errors
/// Returns the refusal of the value under the reading.
fn coerce(value: &Value, reading: ValueReading, read: impl FnOnce(&Value) -> Option<Value>) -> Result<Option<Value>, AttributeRefusal> {
    match read(value) {
        Some(read) => Ok(Some(read)),
        None => AttributeRefusal::unless_blank(reading, value),
    }
}

/// Writes a value as text: text as it is, and any other value as its compact JSON text.
fn coerce_string(value: Value) -> Value {
    match &value {
        Value::String(_) => value,
        Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
            Value::String(CompactString::from(value.to_string()))
        }
    }
}

/// Reads a value as the geometry `target` names, under `policy`, as a Value, which may be any JSON
/// object (ETSI GS CIM 009 v1.9.1 clause 4.5.2): a `GeometryCollection` (RFC 7946 clause 3.1.8) is
/// kept as its `GeoJSON` object when no conversion or target type was declared (see
/// [`Value::to_any_geometry`]). A geometry assembled from nested `mappings` is read here too, as the
/// one part the assembled object is.
///
/// # Errors
/// Returns the refusal of a value that spells no geometry (see [`AttributeRefusal::unless_blank`]),
/// or [`AttributeRefusal::Geometry`] naming why the geometry it spells cannot become the declared
/// type.
fn any_geometry(value: &Value, target: GeometryTarget, policy: GeometryPolicy) -> Result<Option<Value>, AttributeRefusal> {
    match value.to_any_geometry(target, &policy)? {
        Some(geometry) => Ok(Some(geometry)),
        None => AttributeRefusal::unless_blank(ValueReading::Geometry(target), value),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, converted, property},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_geometry::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
    };
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::value::types::{Number, Value};
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    #[test]
    fn a_lone_numeric_part_keeps_its_type_under_a_float_transformation() {
        let value = converted(smallvec![json!(25.5)], property(Transformation::Float), None);

        assert_eq!(value, Some(Value::try_from(25.5).unwrap()));
    }

    #[test]
    fn a_numeric_string_is_parsed_to_an_integer() {
        let value = converted(smallvec![json!("7")], property(Transformation::Integer), None);

        assert_eq!(value, Some(Value::Number(Number::Integer(7))));
    }

    #[test]
    fn a_float_string_is_truncated_to_an_integer() {
        let value = converted(smallvec![json!("7.9")], property(Transformation::Integer), None);

        assert_eq!(value, Some(Value::Number(Number::Integer(7))));
    }

    #[test]
    fn a_comma_decimal_string_parses_as_a_float() {
        let value = converted(smallvec![json!("1,5")], property(Transformation::Float), None);

        assert_eq!(value, Some(Value::try_from(1.5).unwrap()));
    }

    #[test]
    fn text_spelling_no_finite_number_under_a_float_transformation_is_refused_quoting_it() {
        assert_eq!(
            apply(smallvec![json!("NaN")], property(Transformation::Float), None),
            Err(AttributeRefusal::unusable(RequiredValue::Float, "NaN"))
        );
    }

    /// Asserts that `part` under `transformation` is refused as not the `required` scalar, quoting
    /// `quoted`.
    fn assert_scalar_refused(part: JsonValue, transformation: Transformation, required: RequiredValue, quoted: &str) {
        assert_eq!(
            apply(smallvec![part], property(transformation), None),
            Err(AttributeRefusal::unusable(required, quoted)),
            "{transformation}"
        );
    }

    /// Asserts that `part` under `transformation` resolves to no value and is not refused.
    fn assert_scalar_absent(part: JsonValue, transformation: Transformation) {
        assert_eq!(converted(smallvec![part], property(transformation), None), None, "{transformation}");
    }

    #[test]
    fn an_integer_part_and_a_whole_float_read_as_an_integer() {
        assert_eq!(
            converted(smallvec![json!("12")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
        assert_eq!(
            converted(smallvec![json!(12.0)], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
    }

    #[test]
    fn a_number_padded_with_whitespace_reads_as_the_number_it_surrounds() {
        assert_eq!(
            converted(smallvec![json!(" 12 ")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
        assert_eq!(
            converted(smallvec![json!("\t2,5 ")], property(Transformation::Float), None),
            Some(Value::try_from(2.5).unwrap())
        );
    }

    #[test]
    fn text_that_spells_no_integer_under_an_integer_transformation_is_refused_quoting_it() {
        assert_scalar_refused(json!("abc"), Transformation::Integer, RequiredValue::Integer, "abc");
        assert_scalar_refused(json!(" 12 apples "), Transformation::Integer, RequiredValue::Integer, "12 apples");
    }

    #[test]
    fn a_structured_value_under_an_integer_transformation_is_refused_quoting_its_rendering() {
        assert_scalar_refused(json!([1]), Transformation::Integer, RequiredValue::Integer, "[1]");
        assert_scalar_refused(json!({"n": 1}), Transformation::Integer, RequiredValue::Integer, r#"{"n":1}"#);
    }

    #[test]
    fn an_integer_out_of_the_signed_64_bit_range_is_refused_quoting_it() {
        assert_scalar_refused(
            json!("99999999999999999999"),
            Transformation::Integer,
            RequiredValue::Integer,
            "99999999999999999999",
        );
    }

    #[test]
    fn text_that_spells_no_number_under_a_float_transformation_is_refused_quoting_it() {
        assert_scalar_refused(json!("x"), Transformation::Float, RequiredValue::Float, "x");
        assert_scalar_refused(json!([1.5]), Transformation::Float, RequiredValue::Float, "[1.5]");
    }

    #[test]
    fn a_boolean_reads_its_spellings_and_a_number_as_its_truthiness() {
        for (part, expected) in [
            (json!("true"), true),
            (json!("No"), false),
            (json!(1), true),
            (json!(0.0), false),
            (json!(false), false),
        ] {
            assert_eq!(
                converted(smallvec![part.clone()], property(Transformation::Boolean), None),
                Some(Value::Boolean(expected)),
                "{part}"
            );
        }
    }

    #[test]
    fn a_value_that_spells_no_boolean_under_a_boolean_transformation_is_refused_quoting_it() {
        assert_scalar_refused(json!("maybe"), Transformation::Boolean, RequiredValue::Boolean, "maybe");
        assert_scalar_refused(json!(["true"]), Transformation::Boolean, RequiredValue::Boolean, r#"["true"]"#);
    }

    #[test]
    fn blank_text_under_a_scalar_transformation_is_absent_rather_than_refused() {
        for transformation in [Transformation::Boolean, Transformation::Integer, Transformation::Float] {
            assert_scalar_absent(json!(""), transformation);
            assert_scalar_absent(json!("   "), transformation);
        }
    }

    #[test]
    fn a_null_part_under_a_scalar_transformation_is_absent_rather_than_refused() {
        for transformation in [Transformation::Boolean, Transformation::Integer, Transformation::Float] {
            assert_scalar_absent(json!(null), transformation);
        }
    }

    #[test]
    fn several_parts_under_a_scalar_transformation_read_the_first_present_one() {
        assert_eq!(
            converted(smallvec![json!(null), json!("7"), json!("x")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(7)))
        );
        assert_eq!(
            apply(smallvec![json!(null), json!("x"), json!("7")], property(Transformation::Integer), None),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, "x"))
        );
    }

    #[test]
    fn a_numeric_epoch_reads_as_the_same_instant_as_its_text() {
        for transformation in [Transformation::DateTime, Transformation::Date, Transformation::Time] {
            let number = converted(smallvec![json!(1_775_253_620)], property(transformation), None);
            let text = converted(smallvec![json!("1775253620")], property(transformation), None);

            assert!(matches!(number, Some(Value::Temporal(_))), "{transformation:?} left {number:?}");
            assert_eq!(number, text);
        }
    }

    #[test]
    fn a_fractional_numeric_epoch_reads_as_an_instant() {
        let value = converted(smallvec![json!(1_775_253_620.5)], property(Transformation::DateTime), None);

        assert!(matches!(value, Some(Value::Temporal(_))));
    }

    #[test]
    fn a_number_too_small_for_an_epoch_is_refused_quoting_it() {
        let refusal = apply(smallvec![json!(2026)], property(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "2026"));
    }

    #[test]
    fn a_boolean_under_a_temporal_transformation_is_refused_quoting_it() {
        let refusal = apply(smallvec![json!(true)], property(Transformation::DateTime), None);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "true"));
    }

    #[test]
    fn a_blank_temporal_source_is_still_absent_rather_than_refused() {
        let value = converted(smallvec![json!("  ")], property(Transformation::DateTime), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_point_transformation_builds_a_geospatial_value() {
        let value = converted(smallvec![json!(14.5), json!(46.0)], property(Transformation::Point), None);

        assert!(matches!(value, Some(Value::Geospatial(_))));
    }

    #[test]
    fn a_geometry_transformation_keeps_an_already_formed_geometry() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = converted(smallvec![source], property(Transformation::Geometry), None);

        assert!(matches!(value, Some(Value::Geospatial(_))));
    }

    #[test]
    fn a_geometry_transformation_on_text_that_is_no_geometry_is_refused_quoting_the_text() {
        assert_eq!(
            apply(smallvec![json!("not a geometry")], property(Transformation::Geometry), None),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, "not a geometry"))
        );
    }

    #[test]
    fn blank_or_null_coordinate_parts_under_a_point_transformation_are_no_value_rather_than_a_refusal() {
        let value = converted(smallvec![json!(""), json!(null)], property(Transformation::Point), None);

        assert_eq!(value, None);
    }

    #[test]
    fn a_coordinate_part_left_blank_beside_one_present_under_a_point_transformation_is_refused_quoting_both() {
        assert_eq!(
            apply(smallvec![json!(""), json!("46.0")], property(Transformation::Point), None),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, r#"["","46.0"]"#))
        );
    }

    #[test]
    fn a_point_source_promotes_to_a_declared_multipoint() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = converted(smallvec![source], property(Transformation::MultiPoint), None);

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
            apply(smallvec![source], property(Transformation::Polygon), None),
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
        let value = converted(smallvec![source], property(Transformation::Polygon), Some(&policy));

        let Some(Value::Geospatial(geometry)) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::Polygon);
    }

    /// A `GeometryCollection` of the points at `points`.
    fn collection_of(points: &[[f64; 2]]) -> JsonValue {
        json!({
            "type": "GeometryCollection",
            "geometries": points.iter().map(|point| json!({"type": "Point", "coordinates": point})).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn geometry_on_a_property_keeps_a_geometry_collection() {
        let collection = collection_of(&[[1.0, 2.0]]);

        assert_eq!(
            // The collection is consumed by the conversion and still needed to build the expected value.
            converted(smallvec![collection.clone()], property(Transformation::Geometry), None),
            Some(Value::try_from(collection).unwrap())
        );
    }

    #[test]
    fn geometry_on_a_property_reads_one_of_the_six_types_as_a_geo_property_does() {
        assert_eq!(
            converted(
                smallvec![json!({"type": "Point", "coordinates": [16.37, 48.2]})],
                property(Transformation::Geometry),
                None
            ),
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [16.37, 48.2].into(),
            })))
        );
    }

    #[test]
    fn geometry_on_a_property_refuses_a_value_that_spells_no_geometry() {
        assert_eq!(
            apply(smallvec![json!("abc")], property(Transformation::Geometry), None),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, "abc"))
        );
    }

    #[test]
    fn geometry_on_a_property_folds_a_collection_its_policy_flattens() {
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Flatten)).build();

        assert_eq!(
            converted(
                smallvec![collection_of(&[[1.0, 2.0], [3.0, 4.0]])],
                property(Transformation::Geometry),
                Some(&policy)
            ),
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::MultiPoint {
                coordinates: vec![[1.0, 2.0].into(), [3.0, 4.0].into()],
            })))
        );
    }

    #[test]
    fn point_on_a_property_builds_a_point_from_coordinate_parts() {
        assert_eq!(
            converted(smallvec![json!("16.37"), json!("48.2")], property(Transformation::Point), None),
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [16.37, 48.2].into(),
            })))
        );
    }

    #[test]
    fn a_geometry_collection_under_a_named_geometry_type_is_refused() {
        assert_eq!(
            apply(smallvec![collection_of(&[[1.0, 2.0]])], property(Transformation::Point), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn a_clockwise_exterior_ring_is_emitted_counterclockwise() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]],
        });
        let value = converted(smallvec![source], property(Transformation::Polygon), None);

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
    fn a_quoted_number_reads_as_the_number_it_quotes() {
        for (text, integer) in [(r#""12""#, 12), ("'12'", 12), (r#"" 12.7 ""#, 12)] {
            assert_eq!(
                converted(smallvec![json!(text)], property(Transformation::Integer), None),
                Some(Value::Number(Number::Integer(integer))),
                "{text}"
            );
        }
        assert_eq!(
            converted(smallvec![json!(r#""3,5""#)], property(Transformation::Float), None),
            Some(Value::try_from(3.5).unwrap())
        );
    }

    #[test]
    fn integer_reads_decimal_comma_text_as_float_does() {
        assert_eq!(
            converted(smallvec![json!("12,7")], property(Transformation::Integer), None),
            Some(Value::Number(Number::Integer(12)))
        );
    }

    #[test]
    fn a_quoted_value_that_is_no_number_is_refused_quoting_it_as_written() {
        assert_eq!(
            apply(smallvec![json!(r#""abc""#)], property(Transformation::Integer), None),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, r#""abc""#))
        );
    }
}

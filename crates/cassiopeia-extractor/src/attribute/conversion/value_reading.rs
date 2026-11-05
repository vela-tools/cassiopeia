use crate::{
    attribute::{
        conversion::readers::{self, SourceParts},
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_geometry::{policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_mapping::conversion::value_reading::{TemporalReading, ValueReading};
use cassiopeia_ngsi_ld::value::{
    omitted_nulls::OmittedNulls,
    parsing::parse_decimal,
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

impl ReadValue {
    /// The JSON this reading stands for inside a structured value, such as a member of an object
    /// nested `mappings` assemble.
    pub(crate) fn into_json(self) -> JsonValue {
        match self {
            ReadValue::Json(json) => json,
            ReadValue::Value(value) => JsonValue::from(value),
        }
    }
}

/// Reads the source parts as one value, the way `reading` names.
///
/// A geometry reading keeps the parts' array structure so their order, the coordinate order,
/// survives (see [`read_geometry`]). A scalar reading takes the first part present, and every other
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
pub(crate) fn read(
    parts: SourceParts,
    reading: ValueReading,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<ReadValue>, AttributeRefusal> {
    let merged = match reading {
        ValueReading::Geometry(target) => return Ok(read_geometry(parts, target, policy, omitted)?.map(ReadValue::Value)),
        ValueReading::Boolean | ValueReading::Integer | ValueReading::Float => readers::merge_first(parts),
        ValueReading::Text | ValueReading::Object | ValueReading::Json | ValueReading::Temporal(_) => readers::merge_text(parts),
    };

    merged.map_or(Ok(None), |value| read_value(value, reading, policy, omitted))
}

/// Reads one value, already merged from the parts or taken as one item of a list, the way
/// `reading` names.
///
/// The object and JSON readings keep raw JSON with every null in it. Every other reading builds a
/// value, which holds no null (ETSI GS CIM 009 v1.9.1 clause 4.5.0): a null nested in what it reads
/// is left out and counted in `omitted`.
pub(crate) fn read_value(
    value: JsonValue,
    reading: ValueReading,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<ReadValue>, AttributeRefusal> {
    match reading {
        ValueReading::Object => Ok(readers::read_object(value)?.map(|object| ReadValue::Json(JsonValue::Object(object)))),
        ValueReading::Json => Ok(readers::decode(value)?.map(ReadValue::Json)),
        ValueReading::Boolean => typed(value, omitted, |value| {
            coerce_scalar(&value, RequiredValue::Boolean, |value| value.to_boolean().map(Value::from))
        }),
        ValueReading::Integer => typed(value, omitted, |value| coerce_scalar(&value, RequiredValue::Integer, coerce_integer)),
        ValueReading::Float => typed(value, omitted, |value| coerce_scalar(&value, RequiredValue::Float, coerce_float)),
        ValueReading::Text => typed(value, omitted, |value| Ok(Some(coerce_string(value)))),
        ValueReading::Temporal(temporal) => typed(value, omitted, |value| coerce_temporal(&value, temporal)),
        ValueReading::Geometry(target) => typed(value, omitted, |value| coerce_geometry(&value, target, policy).map(Some)),
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

/// Reads the source parts as the geometry `target` names, under the mapping's policy.
///
/// The parts carry no value when every one of them is null or blank text: CSV and spreadsheet
/// sources spell an empty field as an empty string, so an empty cell, or a pair of empty coordinate
/// cells, is a value the record does not carry, and naming every one would drown the run. Any other
/// parts are merged (see [`readers::merge_structured`]) and read.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming why the merged value is no geometry the attribute can
/// hold (see [`coerce_geometry`]).
pub(crate) fn read_geometry(
    parts: SourceParts,
    target: GeometryTarget,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<Value>, AttributeRefusal> {
    if parts.iter().all(readers::is_absent) {
        return Ok(None);
    }

    readers::merge_structured(parts)
        .and_then(|merged| Value::from_json(merged, omitted))
        .map_or(Ok(None), |merged| coerce_geometry(&merged, target, policy).map(Some))
}

/// Reads a value as the scalar a transformation names, through `read`.
///
/// A blank value resolves to no value, as an absent source does: CSV and spreadsheet sources spell
/// an empty field as an empty string, so a blank is a value the record does not carry rather than
/// one it carries wrongly. Any other value that `read` finds no scalar in is refused, text and
/// non-text alike (text such as `abc` under `integer`, an array, an object), because a reading that
/// vanishes unannounced is a value the published entity silently lacks.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting the value as not the `required` scalar: text
/// as written, any other value as its rendering.
fn coerce_scalar(value: &Value, required: RequiredValue, read: impl FnOnce(&Value) -> Option<Value>) -> Result<Option<Value>, AttributeRefusal> {
    if let Some(scalar) = read(value) {
        return Ok(Some(scalar));
    }

    value.reported_text().map_or(Ok(None), |text| Err(AttributeRefusal::unusable(required, &text)))
}

/// Reads a value as an integer, parsing numeric text explicitly.
///
/// Text is read without the whitespace around it, as a padded CSV cell such as ` 12 ` spells the
/// number it surrounds. Float-shaped text truncates toward zero through the same conversion the
/// value model uses, so a value outside the `i64` range is no integer rather than saturating.
fn coerce_integer(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            text.parse::<i64>()
                .ok()
                .or_else(|| {
                    text.parse::<f64>()
                        .ok()
                        .and_then(|float| Value::try_from(float).ok())
                        .and_then(|parsed| parsed.to_integer())
                })
                .map(Value::from)
        }
        Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
            value.to_integer().map(Value::from)
        }
    }
}

/// Reads a value as a float, accepting a comma as a decimal separator.
///
/// Text is read without the whitespace around it, as [`coerce_integer`] reads it.
fn coerce_float(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => parse_decimal(text.trim()).and_then(|float| Value::try_from(float).ok()),
        Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
            value.to_float().and_then(|float| Value::try_from(float).ok())
        }
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

/// Reads a value that is there as the geometry `target` names, under the mapping's policy.
///
/// The value is there, so one that spells no geometry at all is refused rather than left out, the
/// way unreadable text under `datetime` is: a `GeoProperty` that vanishes unannounced is what leaves
/// a published entity without its location. A geometry assembled from nested `mappings` is read
/// here too, as the one part the assembled object is.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting a value that spells no geometry, text as
/// written and any other value as its rendering, or [`AttributeRefusal::Geometry`] naming why the
/// geometry it spells cannot become the declared type.
fn coerce_geometry(value: &Value, target: GeometryTarget, policy: Option<&GeometryPolicy>) -> Result<Value, AttributeRefusal> {
    let policy = policy.copied().unwrap_or_default();
    match value.to_geometry(target, &policy)? {
        Some(geometry) => Ok(Value::Geospatial(Box::new(geometry))),
        None => Err(AttributeRefusal::unusable(
            RequiredValue::Geometry,
            &value.as_str().map_or_else(|| value.to_string(), str::to_owned),
        )),
    }
}

/// Reads a value as a temporal value of the given kind.
///
/// A blank value resolves to no value, as an absent source does: CSV and spreadsheet sources spell
/// an empty field as an empty string, so a blank is a value the record does not carry rather than
/// one it carries wrongly, and naming every empty cell of a column would drown the run. Any other
/// value that is there and will not read is refused instead, text and non-text alike (a number too
/// small to be an epoch, a boolean, an array), because a timestamp that vanishes unannounced is what
/// leaves a published entity with nothing anchoring it in time.
///
/// # Errors
/// Returns [`AttributeRefusal::UnreadableTimestamp`] quoting the value that would not read: text as
/// written, any other value as its rendering.
fn coerce_temporal(value: &Value, kind: TemporalReading) -> Result<Option<Value>, AttributeRefusal> {
    if let Some(datetime) = value.try_parse_datetime() {
        let temporal = match kind {
            TemporalReading::DateTime => TemporalValue::DateTime(datetime),
            TemporalReading::Date => TemporalValue::Date(datetime),
            TemporalReading::Time => TemporalValue::Time(datetime),
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

    #[test]
    fn a_source_geometry_collection_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            apply(smallvec![source], property(Transformation::Geometry), None),
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
}

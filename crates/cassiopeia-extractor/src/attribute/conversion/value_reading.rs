use crate::{
    attribute::{
        conversion::{
            identifier::{self, Identifier},
            json_text,
            part_merge::PartMerge,
            read_context::ReadContext,
            source_parts::{SourceParts, is_absent},
            value_read::{Absence, ReadValue, ValueRead},
        },
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_geometry::{policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, reported_text::is_blank, types::Value};
use serde_json::Value as JsonValue;

/// Reads the source parts as one value, the way `reading` names, or `Ok(None)` when the source
/// carried no value.
///
/// The parts are merged as the reading merges them (see [`PartMerge`]), and the merged value is
/// then read on its own (see [`read_value`]), reading as no value whichever way it carried nothing.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the reading could not use.
pub(crate) fn read(parts: SourceParts, reading: ValueReading, ctx: &mut ReadContext<'_>) -> Result<Option<ReadValue>, AttributeRefusal> {
    PartMerge::from(reading)
        .merge(parts)
        .map_or(Ok(None), |value| Ok(read_value(value, reading, ctx)?.into_read()))
}

/// Reads one value, already merged from the parts or taken as one item of a list, the way
/// `reading` names, saying why when it carried nothing to read.
///
/// A null value carries nothing, and so does blank text under every reading but text, which keeps
/// it as it is; the `json` reading reads text spelling `null` as the null it spells. The object and
/// JSON readings keep raw JSON with every null in it. Every other reading builds a value, which
/// holds no null (ETSI GS CIM 009 v1.9.1 clause 4.5.0): a null nested in what it reads is left out
/// and counted in the context.
///
/// A temporal reading refuses text that reads as no supported spelling of a date-time; a boolean,
/// integer, or float reading refuses a value that spells no such scalar; the object and JSON
/// readings refuse text that does not read as the JSON they ask for, and the object reading any
/// other value that is no object; an identifier reading refuses a value that is not the identifier;
/// and a geometry reading refuses a value that spells no geometry, a geometry the declared type
/// cannot hold, or one this mapping did not authorise converting. The text reading never refuses a
/// value.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the reading could not use.
pub(crate) fn read_value(value: JsonValue, reading: ValueReading, ctx: &mut ReadContext<'_>) -> Result<ValueRead, AttributeRefusal> {
    let value = match value {
        JsonValue::Null => return Ok(ValueRead::Absent(Absence::Null)),
        JsonValue::String(text) if is_blank(&text) && !matches!(reading, ValueReading::Text) => return Ok(ValueRead::Absent(Absence::Blank(text))),
        present @ (JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Array(_) | JsonValue::Object(_)) => present,
    };

    match reading {
        ValueReading::Object => Ok(ValueRead::Read(ReadValue::Json(JsonValue::Object(json_text::object_of(
            value,
            RequiredValue::JsonObject,
        )?)))),
        ValueReading::Json => Ok(match json_text::json_of(value)? {
            JsonValue::Null => ValueRead::Absent(Absence::SpelledNull),
            json @ (JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Array(_) | JsonValue::Object(_)) => {
                ValueRead::Read(ReadValue::Json(json))
            }
        }),
        ValueReading::Text => built(value, ctx.omitted, |value| Ok(Value::String(value.into_text()))),
        ValueReading::Boolean => built(value, ctx.omitted, |value| found(value.to_boolean().map(Value::from), reading, &value)),
        ValueReading::Integer => built(value, ctx.omitted, |value| found(value.to_integer().map(Value::from), reading, &value)),
        ValueReading::Float => built(value, ctx.omitted, |value| {
            found(value.to_float().and_then(|float| Value::try_from(float).ok()), reading, &value)
        }),
        ValueReading::Temporal(kind) => built(value, ctx.omitted, |value| found(value.to_temporal(kind).map(Value::Temporal), reading, &value)),
        ValueReading::Identifier(identifier) => built(value, ctx.omitted, |value| identifier::read_identifier(&value, identifier).map(Value::from)),
        ValueReading::Geometry(target) => {
            let policy = ctx.policy;
            built(value, ctx.omitted, |value| any_geometry(&value, target, policy))
        }
    }
}

/// Builds a value from raw JSON the record carries, leaving out and counting every null nested in
/// it, and reads it through `read`.
///
/// # Errors
/// Returns the refusal `read` names.
fn built(value: JsonValue, omitted: &mut OmittedNulls, read: impl FnOnce(Value) -> Result<Value, AttributeRefusal>) -> Result<ValueRead, AttributeRefusal> {
    match Value::from_json(value, omitted) {
        Some(value) => Ok(ValueRead::Read(ReadValue::Value(read(value)?))),
        None => Ok(ValueRead::Absent(Absence::Null)),
    }
}

/// What `reading` found in `value`, or the refusal of the value when it found nothing (see
/// [`AttributeRefusal::unreadable`]).
///
/// # Errors
/// Returns the refusal of the value under the reading, quoting it (see [`Value::quoted`]).
fn found(found: Option<Value>, reading: ValueReading, value: &Value) -> Result<Value, AttributeRefusal> {
    found.ok_or_else(|| AttributeRefusal::unreadable(reading, &value.quoted()))
}

/// Reads the source parts as the identifier `reading` names, merged into text as a text reading
/// merges them, or `Ok(None)` when the source carried no value: null, or blank text.
///
/// A `Property` reads an identifier the same way, through [`read_value`]; this keeps the identifier
/// as the type it was parsed as, for the type that holds one.
///
/// # Errors
/// Returns the refusal of a value that is not the identifier (see [`identifier::read_identifier`]).
pub(crate) fn read_identifier(parts: SourceParts, reading: IdentifierReading, omitted: &mut OmittedNulls) -> Result<Option<Identifier>, AttributeRefusal> {
    PartMerge::Text
        .merge(parts)
        .filter(|value| !is_absent(value))
        .and_then(|value| Value::from_json(value, omitted))
        .map(|value| identifier::read_identifier(&value, reading))
        .transpose()
}

/// Reads the source parts as text: text as it is, and any other value as its compact JSON text (see
/// [`Value::into_text`]).
///
/// The text reading never refuses a value. A null nested in a value written as text is left out of
/// the text and counted in `omitted`.
pub(crate) fn read_text(parts: SourceParts, omitted: &mut OmittedNulls) -> Option<Value> {
    PartMerge::Text
        .merge(parts)
        .and_then(|value| Value::from_json(value, omitted))
        .map(|value| Value::String(value.into_text()))
}

/// Reads a value the record carries as the geometry `target` names, under `policy`, as a Value,
/// which may be any JSON object (ETSI GS CIM 009 v1.9.1 clause 4.5.2): a `GeometryCollection` (RFC
/// 7946 clause 3.1.8) is kept as its `GeoJSON` object when no conversion or target type was declared
/// (see [`Value::to_any_geometry`]). A geometry assembled from nested `mappings` is read here too, as
/// the one part the assembled object is.
///
/// # Errors
/// Returns the refusal of a value that spells no geometry (see [`AttributeRefusal::unreadable`]), or
/// [`AttributeRefusal::Geometry`] naming why the geometry it spells cannot become the declared type.
fn any_geometry(value: &Value, target: GeometryTarget, policy: GeometryPolicy) -> Result<Value, AttributeRefusal> {
    found(value.to_any_geometry(target, &policy)?, ValueReading::Geometry(target), value)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::{
                read_context::ReadContext,
                source_parts::SourceParts,
                value_read::{Absence, ReadValue, ValueRead},
                value_reading::{read, read_value},
            },
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_geometry::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
        target::GeometryTarget,
    };
    use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
    use cassiopeia_ngsi_ld::value::{
        omitted_nulls::OmittedNulls,
        types::{Number, TemporalValueType, Value},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    /// The point reading, which builds a `Point` from bare coordinates.
    const POINT: ValueReading = ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Point));

    /// The geometry reading that keeps the geometry the source spells.
    const GEOMETRY: ValueReading = ValueReading::Geometry(GeometryTarget::Preserve);

    /// The date-time reading.
    const DATE_TIME: ValueReading = ValueReading::Temporal(TemporalValueType::DateTime);

    /// What reading the parts the way `reading` names, under `policy`, found.
    fn read_under(parts: SourceParts, reading: ValueReading, policy: GeometryPolicy) -> Result<Option<ReadValue>, AttributeRefusal> {
        let mut omitted = OmittedNulls::default();
        read(parts, reading, &mut ReadContext { policy, omitted: &mut omitted })
    }

    /// What reading the parts the way `reading` names, under the default policy, found.
    fn read_as(parts: SourceParts, reading: ValueReading) -> Result<Option<ReadValue>, AttributeRefusal> {
        read_under(parts, reading, GeometryPolicy::default())
    }

    /// The integer value `integer`.
    fn integer(integer: i64) -> Value {
        Value::Number(Number::Integer(integer))
    }

    /// The point geometry value at `coordinates`.
    fn point(coordinates: [f64; 2]) -> Value {
        Value::Geospatial(Box::new(NgsiLdGeometry::Point {
            coordinates: coordinates.into(),
        }))
    }

    #[test]
    fn a_lone_numeric_part_keeps_its_type_under_the_float_reading() {
        assert_eq!(
            read_as(smallvec![json!(25.5)], ValueReading::Float),
            Ok(Some(ReadValue::Value(Value::try_from(25.5).unwrap())))
        );
    }

    #[test]
    fn a_numeric_string_is_parsed_to_an_integer() {
        assert_eq!(read_as(smallvec![json!("7")], ValueReading::Integer), Ok(Some(ReadValue::Value(integer(7)))));
    }

    #[test]
    fn a_float_string_is_truncated_to_an_integer() {
        assert_eq!(read_as(smallvec![json!("7.9")], ValueReading::Integer), Ok(Some(ReadValue::Value(integer(7)))));
    }

    #[test]
    fn a_comma_decimal_string_parses_as_a_float() {
        assert_eq!(
            read_as(smallvec![json!("1,5")], ValueReading::Float),
            Ok(Some(ReadValue::Value(Value::try_from(1.5).unwrap())))
        );
    }

    #[test]
    fn text_spelling_no_finite_number_under_the_float_reading_is_refused_quoting_it() {
        assert_eq!(
            read_as(smallvec![json!("NaN")], ValueReading::Float),
            Err(AttributeRefusal::unusable(RequiredValue::Float, "NaN"))
        );
    }

    /// Asserts that `part` under `reading` is refused as not the `required` scalar, quoting
    /// `quoted`.
    fn assert_scalar_refused(part: JsonValue, reading: ValueReading, required: RequiredValue, quoted: &str) {
        assert_eq!(
            read_as(smallvec![part], reading),
            Err(AttributeRefusal::unusable(required, quoted)),
            "{reading:?}"
        );
    }

    #[test]
    fn an_integer_part_and_a_whole_float_read_as_an_integer() {
        assert_eq!(read_as(smallvec![json!("12")], ValueReading::Integer), Ok(Some(ReadValue::Value(integer(12)))));
        assert_eq!(read_as(smallvec![json!(12.0)], ValueReading::Integer), Ok(Some(ReadValue::Value(integer(12)))));
    }

    #[test]
    fn a_number_padded_with_whitespace_reads_as_the_number_it_surrounds() {
        assert_eq!(
            read_as(smallvec![json!(" 12 ")], ValueReading::Integer),
            Ok(Some(ReadValue::Value(integer(12))))
        );
        assert_eq!(
            read_as(smallvec![json!("\t2,5 ")], ValueReading::Float),
            Ok(Some(ReadValue::Value(Value::try_from(2.5).unwrap())))
        );
    }

    #[test]
    fn text_that_spells_no_integer_under_the_integer_reading_is_refused_quoting_it() {
        assert_scalar_refused(json!("abc"), ValueReading::Integer, RequiredValue::Integer, "abc");
        assert_scalar_refused(json!(" 12 apples "), ValueReading::Integer, RequiredValue::Integer, "12 apples");
    }

    #[test]
    fn a_structured_value_under_the_integer_reading_is_refused_quoting_its_rendering() {
        assert_scalar_refused(json!([1]), ValueReading::Integer, RequiredValue::Integer, "[1]");
        assert_scalar_refused(json!({"n": 1}), ValueReading::Integer, RequiredValue::Integer, r#"{"n":1}"#);
    }

    #[test]
    fn an_integer_out_of_the_signed_64_bit_range_is_refused_quoting_it() {
        assert_scalar_refused(
            json!("99999999999999999999"),
            ValueReading::Integer,
            RequiredValue::Integer,
            "99999999999999999999",
        );
    }

    #[test]
    fn text_that_spells_no_number_under_the_float_reading_is_refused_quoting_it() {
        assert_scalar_refused(json!("x"), ValueReading::Float, RequiredValue::Float, "x");
        assert_scalar_refused(json!([1.5]), ValueReading::Float, RequiredValue::Float, "[1.5]");
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
            // The part is consumed by the reading and still needed to name the failing case.
            assert_eq!(
                read_as(smallvec![part.clone()], ValueReading::Boolean),
                Ok(Some(ReadValue::Value(Value::Boolean(expected)))),
                "{part}"
            );
        }
    }

    #[test]
    fn a_value_that_spells_no_boolean_under_the_boolean_reading_is_refused_quoting_it() {
        assert_scalar_refused(json!("maybe"), ValueReading::Boolean, RequiredValue::Boolean, "maybe");
        assert_scalar_refused(json!(["true"]), ValueReading::Boolean, RequiredValue::Boolean, r#"["true"]"#);
    }

    #[test]
    fn blank_text_under_every_reading_but_text_is_absent_rather_than_refused() {
        for reading in [
            ValueReading::Boolean,
            ValueReading::Integer,
            ValueReading::Float,
            ValueReading::Identifier(IdentifierReading::Iri),
            ValueReading::Identifier(IdentifierReading::Urn),
            DATE_TIME,
            ValueReading::Object,
            ValueReading::Json,
            POINT,
            GEOMETRY,
        ] {
            assert_eq!(read_as(smallvec![json!("")], reading), Ok(None), "{reading:?}");
            assert_eq!(read_as(smallvec![json!("   ")], reading), Ok(None), "{reading:?}");
        }
    }

    #[test]
    fn a_null_part_under_a_scalar_reading_is_absent_rather_than_refused() {
        for reading in [ValueReading::Boolean, ValueReading::Integer, ValueReading::Float] {
            assert_eq!(read_as(smallvec![JsonValue::Null], reading), Ok(None), "{reading:?}");
        }
    }

    #[test]
    fn several_parts_under_a_scalar_reading_read_the_first_present_one() {
        assert_eq!(
            read_as(smallvec![JsonValue::Null, json!("7"), json!("x")], ValueReading::Integer),
            Ok(Some(ReadValue::Value(integer(7))))
        );
        assert_eq!(
            read_as(smallvec![JsonValue::Null, json!("x"), json!("7")], ValueReading::Integer),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, "x"))
        );
    }

    #[test]
    fn a_numeric_epoch_reads_as_the_same_instant_as_its_text() {
        for kind in [TemporalValueType::DateTime, TemporalValueType::Date, TemporalValueType::Time] {
            let number = read_as(smallvec![json!(1_775_253_620)], ValueReading::Temporal(kind));
            let text = read_as(smallvec![json!("1775253620")], ValueReading::Temporal(kind));

            assert!(matches!(number, Ok(Some(ReadValue::Value(Value::Temporal(_))))), "{kind:?} left {number:?}");
            assert_eq!(number, text);
        }
    }

    #[test]
    fn a_fractional_numeric_epoch_reads_as_an_instant() {
        assert!(matches!(
            read_as(smallvec![json!(1_775_253_620.5)], DATE_TIME),
            Ok(Some(ReadValue::Value(Value::Temporal(_))))
        ));
    }

    #[test]
    fn a_number_too_small_for_an_epoch_is_refused_quoting_it() {
        let refusal = read_as(smallvec![json!(2026)], DATE_TIME);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "2026"));
    }

    #[test]
    fn a_boolean_under_a_temporal_reading_is_refused_quoting_it() {
        let refusal = read_as(smallvec![json!(true)], DATE_TIME);

        assert!(matches!(refusal, Err(AttributeRefusal::UnreadableTimestamp { ref text }) if text.as_ref() == "true"));
    }

    #[test]
    fn a_geometry_reading_keeps_an_already_formed_geometry() {
        assert_eq!(
            read_as(smallvec![json!({"type": "Point", "coordinates": [9.17, 45.47]})], GEOMETRY),
            Ok(Some(ReadValue::Value(point([9.17, 45.47]))))
        );
    }

    #[test]
    fn a_geometry_reading_refuses_a_value_that_spells_no_geometry_quoting_it() {
        assert_eq!(
            read_as(smallvec![json!("not a geometry")], GEOMETRY),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, "not a geometry"))
        );
    }

    #[test]
    fn the_point_reading_builds_a_point_from_coordinate_parts() {
        assert_eq!(
            read_as(smallvec![json!("16.37"), json!("48.2")], POINT),
            Ok(Some(ReadValue::Value(point([16.37, 48.2]))))
        );
    }

    #[test]
    fn blank_or_null_coordinate_parts_under_the_point_reading_are_no_value_rather_than_a_refusal() {
        assert_eq!(read_as(smallvec![json!(""), JsonValue::Null], POINT), Ok(None));
    }

    #[test]
    fn a_coordinate_part_left_blank_beside_one_present_under_the_point_reading_is_refused_quoting_both() {
        assert_eq!(
            read_as(smallvec![json!(""), json!("46.0")], POINT),
            Err(AttributeRefusal::unusable(RequiredValue::Geometry, r#"["","46.0"]"#))
        );
    }

    /// A `MultiPolygon` of two squares, the second of side `side`.
    fn two_squares(side: f64) -> JsonValue {
        let far = 5.0 + side;
        json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [far, 5.0], [far, far], [5.0, 5.0]]],
            ],
        })
    }

    #[test]
    fn a_geometry_the_declared_type_cannot_hold_is_refused_naming_why() {
        assert_eq!(
            read_as(
                smallvec![two_squares(1.0)],
                ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Polygon))
            ),
            Err(AttributeRefusal::Geometry(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            }))
        );
    }

    #[test]
    fn a_geometry_reading_converts_under_the_declared_policy() {
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Largest)).build();

        assert!(matches!(
            read_under(
                smallvec![two_squares(3.0)],
                ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Polygon)),
                policy
            ),
            Ok(Some(ReadValue::Value(Value::Geospatial(geometry)))) if geometry.kind() == GeometryKind::Polygon
        ));
    }

    #[test]
    fn a_quoted_number_reads_as_the_number_it_quotes() {
        for (text, expected) in [(r#""12""#, 12), ("'12'", 12), (r#"" 12.7 ""#, 12)] {
            assert_eq!(
                read_as(smallvec![json!(text)], ValueReading::Integer),
                Ok(Some(ReadValue::Value(integer(expected)))),
                "{text}"
            );
        }
        assert_eq!(
            read_as(smallvec![json!(r#""3,5""#)], ValueReading::Float),
            Ok(Some(ReadValue::Value(Value::try_from(3.5).unwrap())))
        );
    }

    #[test]
    fn the_integer_reading_reads_decimal_comma_text_as_the_float_reading_does() {
        assert_eq!(
            read_as(smallvec![json!("12,7")], ValueReading::Integer),
            Ok(Some(ReadValue::Value(integer(12))))
        );
    }

    #[test]
    fn a_quoted_value_that_is_no_number_is_refused_quoting_it_as_written() {
        assert_eq!(
            read_as(smallvec![json!(r#""abc""#)], ValueReading::Integer),
            Err(AttributeRefusal::unusable(RequiredValue::Integer, r#""abc""#))
        );
    }

    #[test]
    fn an_identifier_reading_reads_the_normalized_identifier_as_text() {
        assert_eq!(
            read_as(smallvec![json!(" URN:Ex:a ")], ValueReading::Identifier(IdentifierReading::Urn)),
            Ok(Some(ReadValue::Value(Value::String("urn:ex:a".into()))))
        );
    }

    /// What reading `value` the way `reading` names found.
    fn read_one(value: JsonValue, reading: ValueReading) -> Result<ValueRead, AttributeRefusal> {
        let mut omitted = OmittedNulls::default();
        read_value(
            value,
            reading,
            &mut ReadContext {
                policy: GeometryPolicy::default(),
                omitted: &mut omitted,
            },
        )
    }

    #[test]
    fn reading_one_value_says_why_it_carried_nothing() {
        assert_eq!(read_one(JsonValue::Null, ValueReading::Integer), Ok(ValueRead::Absent(Absence::Null)));
        assert_eq!(
            read_one(json!(" "), ValueReading::Identifier(IdentifierReading::Iri)),
            Ok(ValueRead::Absent(Absence::Blank(" ".to_owned())))
        );
        assert_eq!(read_one(json!("null"), ValueReading::Json), Ok(ValueRead::Absent(Absence::SpelledNull)));
    }

    #[test]
    fn the_text_reading_keeps_blank_text_as_it_is() {
        assert_eq!(
            read_one(json!(" "), ValueReading::Text),
            Ok(ValueRead::Read(ReadValue::Value(Value::String(" ".into()))))
        );
    }

    #[test]
    fn the_object_reading_refuses_text_spelling_null() {
        assert_eq!(
            read_one(json!("null"), ValueReading::Object),
            Err(AttributeRefusal::unusable(RequiredValue::JsonObject, "null"))
        );
    }
}

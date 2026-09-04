use crate::attribute::refusal::AttributeRefusal;
use cassiopeia_geometry::{error::GeometryError, policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_mapping::transformation::Transformation;
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
    /// Applies a transformation to the source values collected for one attribute.
    ///
    /// An absent transformation defaults to `String`, matching a mapping that names a source and no
    /// conversion. `Array` aggregates all parts; every other transformation first merges the parts
    /// into one intermediate value and then coerces it to the target NGSI-LD type.
    ///
    /// Two transformations can refuse. A transformation naming a geometry type routes through the
    /// geometry lattice, where the source may carry a geometry a `GeoProperty` cannot hold or one
    /// this mapping did not authorise converting; a temporal transformation refuses text that reads
    /// as no supported spelling of a date-time. Every other transformation drops an unusable value
    /// to null and cannot fail.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the attribute could not take.
    pub(crate) fn apply(parts: SourceParts, transformation: Option<&Transformation>, geometry: Option<&GeometryPolicy>) -> Result<Value, AttributeRefusal> {
        let transformation = transformation.copied().unwrap_or(Transformation::String);

        match transformation.geometry_target() {
            Some(target) => Ok(Self::coerce_geometry(&Self::merge_geometry(parts), target, geometry)?),
            None => Self::coerce_value(parts, transformation),
        }
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
            Transformation::Object => Ok(Self::coerce_object(&Self::merge_text(parts))),
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

    /// Flattens the parts into one array, dropping nulls and splicing in any nested arrays.
    fn aggregate(parts: SourceParts) -> Value {
        let mut result = Vec::new();
        for part in parts.into_iter().filter(|part| !part.is_null()) {
            match part {
                JsonValue::Array(array) => result.extend(array),
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Object(_) => {
                    result.push(part);
                }
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

    /// Merges parts for a geometry target: a lone part keeps its type, several keep their array
    /// structure so the coordinate order survives.
    fn merge_geometry(mut parts: SourceParts) -> Value {
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

    /// Coerces the merged value to a JSON object, treating an empty object as null.
    fn coerce_object(value: &Value) -> Value {
        if value.is_null() {
            return Value::Null;
        }

        let object = value.to_object();
        if object.is_empty() { Value::Null } else { Value::Object(Box::new(object)) }
    }

    /// Reads the merged value as the geometry the transformation names, under the mapping's policy.
    ///
    /// A value that carries no geometry at all yields a null, so the attribute is simply omitted, the
    /// way an absent source field always has been. A value that carries a geometry which cannot
    /// legally become the declared type is refused instead, so the run can say what it dropped and
    /// why.
    fn coerce_geometry(value: &Value, target: GeometryTarget, policy: Option<&GeometryPolicy>) -> Result<Value, GeometryError> {
        let policy = policy.copied().unwrap_or_default();
        let geometry = value.to_geometry(target, &policy)?;

        Ok(geometry.map_or(Value::Null, |geometry| Value::Geospatial(Box::new(geometry))))
    }

    /// Coerces the merged value to a temporal value of the given kind.
    ///
    /// An absent source resolves to null, and so does a blank one: CSV and spreadsheet sources spell
    /// an empty field as an empty string, so a blank is a value the record does not carry rather
    /// than one it carries wrongly, and naming every empty cell of a column would drown the run.
    /// Text that is there and will not read is refused instead, because a timestamp that vanishes
    /// unannounced is what leaves a published entity with nothing anchoring it in time.
    ///
    /// A value carrying no text at all resolves to null: there is no spelling to quote back, so
    /// there is nothing a mapping author could act on.
    ///
    /// # Errors
    /// Returns [`AttributeRefusal::UnreadableTimestamp`] carrying the text that would not read.
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

        match value.as_str().map(str::trim).filter(|text| !text.is_empty()) {
            Some(text) => Err(AttributeRefusal::UnreadableTimestamp { text: Box::from(text) }),
            None => Ok(Value::Null),
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
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::value::types::{Number, Value};
    use serde_json::json;
    use smallvec::smallvec;

    #[test]
    fn a_lone_numeric_part_keeps_its_type_under_a_float_transformation() {
        let value = Transformer::apply(smallvec![json!(25.5)], Some(&Transformation::Float), None).unwrap();

        assert_eq!(value, Value::Number(Number::Float(25.5)));
    }

    #[test]
    fn a_numeric_string_is_parsed_to_an_integer() {
        let value = Transformer::apply(smallvec![json!("7")], Some(&Transformation::Integer), None).unwrap();

        assert_eq!(value, Value::Number(Number::Integer(7)));
    }

    #[test]
    fn a_float_string_is_truncated_to_an_integer() {
        let value = Transformer::apply(smallvec![json!("7.9")], Some(&Transformation::Integer), None).unwrap();

        assert_eq!(value, Value::Number(Number::Integer(7)));
    }

    #[test]
    fn a_comma_decimal_string_parses_as_a_float() {
        let value = Transformer::apply(smallvec![json!("1,5")], Some(&Transformation::Float), None).unwrap();

        assert_eq!(value, Value::Number(Number::Float(1.5)));
    }

    #[test]
    fn several_string_parts_are_concatenated() {
        let value = Transformer::apply(smallvec![json!("Station-"), json!(42)], Some(&Transformation::String), None).unwrap();

        assert_eq!(value, Value::String("Station-42".into()));
    }

    #[test]
    fn string_parts_past_the_inline_capacity_are_concatenated_in_order() {
        // Four parts spill the inline capacity of one onto the heap; order must survive the spill.
        let value = Transformer::apply(
            smallvec![json!("Station-"), json!(42), json!("/"), json!("north")],
            Some(&Transformation::String),
            None,
        )
        .unwrap();

        assert_eq!(value, Value::String("Station-42/north".into()));
    }

    #[test]
    fn the_default_transformation_is_string() {
        let value = Transformer::apply(smallvec![json!(true)], None, None).unwrap();

        assert_eq!(value, Value::String("true".into()));
    }

    #[test]
    fn an_array_transformation_flattens_and_drops_nulls() {
        let value = Transformer::apply(smallvec![json!([1, 2]), json!(null), json!(3)], Some(&Transformation::Array), None).unwrap();

        assert_eq!(value, Value::from(json!([1, 2, 3])));
    }

    #[test]
    fn an_empty_array_transformation_is_null() {
        let value = Transformer::apply(smallvec![json!(null)], Some(&Transformation::Array), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn a_point_transformation_builds_a_geospatial_value() {
        let value = Transformer::apply(smallvec![json!(14.5), json!(46.0)], Some(&Transformation::Point), None).unwrap();

        assert!(matches!(value, Value::Geospatial(_)));
    }

    #[test]
    fn a_geometry_transformation_keeps_an_already_formed_geometry() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = Transformer::apply(smallvec![source], Some(&Transformation::Geometry), None).unwrap();

        assert!(matches!(value, Value::Geospatial(_)));
    }

    #[test]
    fn a_geometry_transformation_on_a_non_geometry_value_is_null() {
        let value = Transformer::apply(smallvec![json!("not a geometry")], Some(&Transformation::Geometry), None).unwrap();

        assert!(value.is_null());
    }

    #[test]
    fn a_point_source_promotes_to_a_declared_multipoint() {
        let source = json!({"type": "Point", "coordinates": [9.17, 45.47]});
        let value = Transformer::apply(smallvec![source], Some(&Transformation::MultiPoint), None).unwrap();

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
            Transformer::apply(smallvec![source], Some(&Transformation::Polygon), None),
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
        let value = Transformer::apply(smallvec![source], Some(&Transformation::Polygon), Some(&policy)).unwrap();

        let Value::Geospatial(geometry) = value else {
            panic!("a geometry");
        };
        assert_eq!(geometry.kind(), GeometryKind::Polygon);
    }

    #[test]
    fn a_source_geometry_collection_is_refused() {
        let source = json!({"type": "GeometryCollection", "geometries": []});

        assert_eq!(
            Transformer::apply(smallvec![source], Some(&Transformation::Geometry), None),
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }

    #[test]
    fn a_clockwise_exterior_ring_is_emitted_counterclockwise() {
        let source = json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]],
        });
        let value = Transformer::apply(smallvec![source], Some(&Transformation::Polygon), None).unwrap();

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
        let value = Transformer::apply(smallvec![json!(null)], Some(&Transformation::Integer), None).unwrap();

        assert!(value.is_null());
    }
}

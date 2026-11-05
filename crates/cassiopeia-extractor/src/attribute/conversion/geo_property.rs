use crate::attribute::{
    conversion::{attribute_conversion::AttributeConversion, part_merge::PartMerge, read_context::ReadContext, source_parts::SourceParts},
    refusal::AttributeRefusal,
};
use cassiopeia_geometry::target::GeometryTarget;
use cassiopeia_mapping::conversion::value_reading::ValueReading;
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// A `GeoProperty`'s value, a `GeoJSON` geometry of one of the six types ETSI GS CIM 009 v1.9.1
/// clause 4.7.1 admits, built as the target names under the declaration's policy.
///
/// A `GeometryCollection` is none of the six, so it is refused unless the policy folds it into one
/// of them. Reading builds the geometry the type holds, so admitting keeps it as it is.
impl AttributeConversion for GeometryTarget {
    type Content = Value;
    type Reading = Value;

    fn read(self, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<Value>, AttributeRefusal> {
        let Some(value) = PartMerge::Structured.merge(parts).and_then(|merged| Value::from_json(merged, ctx.omitted)) else {
            return Ok(None);
        };

        match value.to_geometry(self, &ctx.policy)? {
            Some(geometry) => Ok(Some(Value::Geospatial(Box::new(geometry)))),
            None => Err(AttributeRefusal::unreadable(ValueReading::Geometry(self), &value.quoted())),
        }
    }

    fn admit(self, reading: Value, _omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
        Ok(Some(reading))
    }

    fn into_json(reading: Value) -> JsonValue {
        JsonValue::from(reading)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, converted, default_of},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_geometry::{error::GeometryError, geometry::NgsiLdGeometry, policy::GeometryPolicy, refusal::GeometryRefusal, strategy::ConversionStrategy};
    use cassiopeia_ngsi_ld::{entity::attribute::NgsiLdAttributeKind, value::types::Value};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use smallvec::smallvec;

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
    fn a_null_or_blank_part_under_the_geo_property_default_is_no_value_rather_than_a_refusal() {
        for part in [json!(""), json!("  "), json!(null)] {
            let value = converted(smallvec![part], default_of(NgsiLdAttributeKind::GeoProperty), None);

            assert_eq!(value, None, "{value:?}");
        }
    }

    #[test]
    fn a_value_spelling_no_geometry_under_the_geo_property_default_is_refused_quoting_it() {
        for (part, quoted) in [
            (json!("not geojson"), "not geojson"),
            (json!({"type": "Point"}), r#"{"type":"Point"}"#),
            (json!(42), "42"),
            (json!(true), "true"),
        ] {
            assert_eq!(
                apply(smallvec![part], default_of(NgsiLdAttributeKind::GeoProperty), None),
                Err(AttributeRefusal::unusable(RequiredValue::Geometry, quoted)),
                "{quoted}"
            );
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
            Err(AttributeRefusal::Geometry(GeometryRefusal::Inadmissible(GeometryError::GeometryCollection)))
        );
    }
}

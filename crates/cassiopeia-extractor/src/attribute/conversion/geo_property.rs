use crate::attribute::{
    conversion::{readers::SourceParts, value_reading},
    refusal::AttributeRefusal,
};
use cassiopeia_geometry::{policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};

/// Converts the parts into a `GeoProperty`'s value, a `GeoJSON` geometry of one of the six types
/// ETSI GS CIM 009 v1.9.1 clause 4.7.1 admits, built as `target` names under the mapping's policy.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming why the parts hold no geometry the attribute can hold
/// (see [`value_reading::read_geometry`]).
pub(crate) fn apply(
    parts: SourceParts,
    target: GeometryTarget,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<Value>, AttributeRefusal> {
    value_reading::read_geometry(parts, target, policy, omitted)
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
    use cassiopeia_geometry::{error::GeometryError, geometry::NgsiLdGeometry, policy::GeometryPolicy, strategy::ConversionStrategy};
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
            Err(AttributeRefusal::Geometry(GeometryError::GeometryCollection))
        );
    }
}

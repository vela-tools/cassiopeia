use crate::value::types::Value;
use cassiopeia_geometry::{
    convert::{convert, from_coordinates, from_geojson},
    coordinates::CoordinateShape,
    error::GeometryError,
    geometry::NgsiLdGeometry,
    policy::GeometryPolicy,
    target::GeometryTarget,
};
use geojson::{Geometry, Position};

impl Value {
    /// Reads the value as the geometry an NGSI-LD `GeoProperty` is to carry.
    ///
    /// Three source shapes are recognised, in order: a geometry the pipeline already typed, a
    /// `GeoJSON` geometry object the source carried as text or as a structured object (RFC 7946
    /// clause 3.1), and, only when the mapping declared which geometry type it wants, a bare
    /// coordinate array.
    ///
    /// Coordinate reading lives here rather than in the geometry crate because it goes through
    /// [`Value::to_float`], which accepts the comma-decimal numeric strings that CSV and other
    /// text-shaped sources write for a longitude or a latitude.
    ///
    /// `Ok(None)` means *absent*: this value carries no geometry, the attribute is simply omitted,
    /// exactly as before. An `Err` means *refused*: the value carries a geometry a `GeoProperty`
    /// cannot hold, or one this mapping did not authorise converting.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] naming the refusal.
    pub fn to_geometry(&self, target: GeometryTarget, policy: &GeometryPolicy) -> Result<Option<NgsiLdGeometry>, GeometryError> {
        if let Value::Geospatial(geometry) = self {
            // The geometry is borrowed from the value and the conversion consumes what it converts,
            // so this is the one copy the typed path makes.
            return convert((**geometry).clone(), target, policy).map(Some);
        }

        if let Some(geometry) = self.as_source_geometry() {
            return from_geojson(geometry, target, policy).map(Some);
        }

        match target {
            // A bare coordinate array carries no geometry type of its own, so there is nothing to
            // preserve; only a declared target says what the numbers mean.
            GeometryTarget::Preserve => Ok(None),
            GeometryTarget::Coerce(kind) => match self.as_coordinate_shape() {
                Some(shape) => from_coordinates(shape, kind, policy).map(Some),
                None => Ok(None),
            },
        }
    }

    /// Reads a `GeoJSON` geometry object the source carried as text or as a structured object.
    fn as_source_geometry(&self) -> Option<Geometry> {
        match self {
            Value::String(text) => serde_json::from_str(text).ok(),
            Value::Object(map) => {
                let json = serde_json::to_value(map.as_ref()).ok()?;
                serde_json::from_value(json).ok()
            }
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) => None,
        }
    }

    /// Reads a bare coordinate array, at whatever depth the source nested it.
    ///
    /// An array whose elements are all readable as numbers is one position; an array of arrays is a
    /// group one level up. An array carrying anything else reads as no coordinates at all, so a
    /// value that merely looks like a coordinate list (`[1.0, 2.0, "junk"]`) is left absent rather
    /// than being truncated into a position the source did not write.
    fn as_coordinate_shape(&self) -> Option<CoordinateShape> {
        let Value::Array(items) = self else {
            return None;
        };

        if let Some(position) = as_position(items) {
            return Some(CoordinateShape::Position(position));
        }

        let members = items.iter().map(Value::as_coordinate_shape).collect::<Option<Vec<CoordinateShape>>>()?;
        if members.is_empty() {
            return None;
        }

        Some(CoordinateShape::Group(members))
    }
}

/// Reads a flat array of numbers as one position (RFC 7946 clause 3.1.1).
///
/// Every element has to read as a number: a position is a coordinate tuple, so one unreadable
/// element makes the whole array something other than a position.
fn as_position(items: &[Value]) -> Option<Position> {
    if items.len() < 2 {
        return None;
    }

    let components = items.iter().map(Value::to_float).collect::<Option<Vec<f64>>>()?;

    Some(Position::from(components))
}

#[cfg(test)]
mod tests {
    use crate::value::types::Value;
    use cassiopeia_geometry::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
        target::GeometryTarget,
    };
    use geojson::Position;
    use serde_json::json;

    fn preserve(value: &Value) -> Result<Option<NgsiLdGeometry>, GeometryError> {
        value.to_geometry(GeometryTarget::Preserve, &GeometryPolicy::default())
    }

    fn coerce(value: &Value, kind: GeometryKind) -> Result<Option<NgsiLdGeometry>, GeometryError> {
        value.to_geometry(GeometryTarget::Coerce(kind), &GeometryPolicy::default())
    }

    #[test]
    fn a_coordinate_pair_becomes_the_declared_point() {
        let value = Value::from(json!([1.0, 2.0]));

        assert_eq!(
            coerce(&value, GeometryKind::Point),
            Ok(Some(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0]),
            }))
        );
    }

    #[test]
    fn a_comma_decimal_coordinate_string_still_reads_as_a_number() {
        // CSV sources routinely write a longitude as `14,5`; the value model parses it, which is why
        // coordinate reading lives here rather than in the geometry crate.
        let value = Value::from(json!(["14,5", "46,05"]));

        assert_eq!(
            coerce(&value, GeometryKind::Point),
            Ok(Some(NgsiLdGeometry::Point {
                coordinates: Position::from([14.5, 46.05]),
            }))
        );
    }

    #[test]
    fn a_coordinate_array_carrying_an_unreadable_element_is_absent_rather_than_truncated() {
        let value = Value::from(json!([1.0, 2.0, "junk"]));

        assert_eq!(coerce(&value, GeometryKind::Point), Ok(None));
    }

    #[test]
    fn an_altitude_survives_a_three_element_coordinate_array() {
        let value = Value::from(json!([1.0, 2.0, 300.0]));

        assert_eq!(
            coerce(&value, GeometryKind::Point),
            Ok(Some(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0, 300.0]),
            }))
        );
    }

    #[test]
    fn a_non_geometry_carries_no_geometry() {
        assert_eq!(coerce(&Value::from(json!("not a point")), GeometryKind::Point), Ok(None));
        assert_eq!(preserve(&Value::from(json!("not a point"))), Ok(None));
        assert_eq!(preserve(&Value::Null), Ok(None));
    }

    #[test]
    fn a_geojson_object_is_preserved_as_the_geometry_it_declares() {
        let value = Value::from(json!({"type": "Point", "coordinates": [1.0, 2.0]}));

        assert_eq!(
            preserve(&value),
            Ok(Some(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0]),
            }))
        );
    }

    #[test]
    fn a_geojson_geometry_written_as_text_is_parsed() {
        let value = Value::String(r#"{"type":"Point","coordinates":[9.17,45.47]}"#.into());

        assert!(matches!(preserve(&value), Ok(Some(NgsiLdGeometry::Point { .. }))));
    }

    #[test]
    fn a_source_geometry_collection_is_refused_rather_than_carried() {
        let value = Value::from(json!({"type": "GeometryCollection", "geometries": []}));

        assert_eq!(preserve(&value), Err(GeometryError::GeometryCollection));
    }

    #[test]
    fn a_source_geometry_collection_folds_when_the_mapping_declares_it() {
        let value = Value::from(json!({
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [1.0, 2.0]},
                {"type": "Point", "coordinates": [3.0, 4.0]},
            ],
        }));
        let policy = GeometryPolicy::builder().convert(Some(ConversionStrategy::Flatten)).build();

        assert_eq!(
            value.to_geometry(GeometryTarget::Preserve, &policy),
            Ok(Some(NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0]), Position::from([3.0, 4.0])],
            }))
        );
    }

    #[test]
    fn a_point_source_promotes_to_a_declared_multipoint() {
        let value = Value::from(json!({"type": "Point", "coordinates": [1.0, 2.0]}));

        assert_eq!(
            coerce(&value, GeometryKind::MultiPoint),
            Ok(Some(NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0])],
            }))
        );
    }

    #[test]
    fn a_multipolygon_source_is_refused_toward_a_declared_polygon_without_a_conversion() {
        let value = Value::from(json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]],
            ],
        }));

        assert_eq!(
            coerce(&value, GeometryKind::Polygon),
            Err(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            })
        );
    }
}

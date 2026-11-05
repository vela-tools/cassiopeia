use crate::value::types::{Value, ValueObject};
use cassiopeia_geometry::{
    collection::{CollectionMember, GeometryCollection},
    convert::{convert, from_coordinates, from_geojson},
    coordinates::CoordinateShape,
    error::GeometryError,
    geometry::NgsiLdGeometry,
    policy::GeometryPolicy,
    target::GeometryTarget,
};
use compact_str::CompactString;
use geojson::{Geometry, GeometryValue, Position};

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

    /// Reads the value as any `GeoJSON` geometry (RFC 7946 clause 3.1), for a Property, whose value
    /// may be any JSON object.
    ///
    /// A `GeoProperty` admits only six geometry types (ETSI GS CIM 009 v1.9.1 clause 4.7.1), so a
    /// `GeometryCollection` (RFC 7946 clause 3.1.8) can reach a broker only as a Property's value.
    /// Kept as the source spells it, with no `geometry` conversion declared and no target type
    /// named, the collection is written as its `GeoJSON` object: each member is read as a geometry
    /// of its own under `policy` (see [`GeometryCollection::from_geojson`]), so it is validated and
    /// normalised as any geometry is. Every other
    /// value is read as [`Value::to_geometry`] reads it, and becomes the typed geometry it spells.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] naming why the value, or a member of the collection it spells,
    /// is no geometry the attribute can hold.
    pub fn to_any_geometry(&self, target: GeometryTarget, policy: &GeometryPolicy) -> Result<Option<Value>, GeometryError> {
        let keeps_collection = matches!(target, GeometryTarget::Preserve) && policy.convert().is_none();
        if !keeps_collection || matches!(self, Value::Geospatial(_)) {
            return Ok(self.to_geometry(target, policy)?.map(|geometry| Value::Geospatial(Box::new(geometry))));
        }

        match self.as_source_geometry() {
            Some(Geometry {
                value: GeometryValue::GeometryCollection { geometries },
                ..
            }) => Ok(Some(Value::from(GeometryCollection::from_geojson(geometries, policy)?))),
            Some(geometry) => from_geojson(geometry, target, policy).map(|geometry| Some(Value::Geospatial(Box::new(geometry)))),
            // A bare coordinate array carries no geometry type of its own, so with no target named
            // there is nothing to preserve.
            None => Ok(None),
        }
    }

    /// The geometry this value holds when it is one the pipeline already typed, and `None` for any
    /// other value.
    ///
    /// Nothing is parsed or converted: a geometry is typed, under its declaration's `geometry`
    /// policy, by the extraction stage that reads it, so a later stage only takes it as it is.
    #[must_use]
    pub fn into_geometry(self) -> Option<NgsiLdGeometry> {
        match self {
            Value::Geospatial(geometry) => Some(*geometry),
            Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Array(_) | Value::Object(_) => None,
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
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) => None,
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

/// Writes a collection as the `GeoJSON` object it is (RFC 7946 clause 3.1.8): its `type` and its
/// `geometries`, each member the geometry it holds, and a nested collection the object it is.
// Manual because the collection becomes an object of two members built from its parts, which no
// derive expresses.
impl From<GeometryCollection> for Value {
    fn from(collection: GeometryCollection) -> Value {
        let geometries = Vec::from(collection)
            .into_iter()
            .map(|member| match member {
                CollectionMember::Geometry(geometry) => Value::Geospatial(Box::new(geometry)),
                CollectionMember::Collection(nested) => Value::from(nested),
            })
            .collect();
        let object = [
            (CompactString::const_new("type"), Value::String(CompactString::const_new("GeometryCollection"))),
            (CompactString::const_new("geometries"), Value::Array(geometries)),
        ]
        .into_iter()
        .collect::<ValueObject>();

        Value::Object(Box::new(object))
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
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn preserve(value: &Value) -> Result<Option<NgsiLdGeometry>, GeometryError> {
        value.to_geometry(GeometryTarget::Preserve, &GeometryPolicy::default())
    }

    fn coerce(value: &Value, kind: GeometryKind) -> Result<Option<NgsiLdGeometry>, GeometryError> {
        value.to_geometry(GeometryTarget::Coerce(kind), &GeometryPolicy::default())
    }

    #[test]
    fn a_coordinate_pair_becomes_the_declared_point() {
        let value = Value::try_from(json!([1.0, 2.0])).unwrap();

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
        let value = Value::try_from(json!(["14,5", "46,05"])).unwrap();

        assert_eq!(
            coerce(&value, GeometryKind::Point),
            Ok(Some(NgsiLdGeometry::Point {
                coordinates: Position::from([14.5, 46.05]),
            }))
        );
    }

    #[test]
    fn a_coordinate_array_carrying_an_unreadable_element_is_absent_rather_than_truncated() {
        let value = Value::try_from(json!([1.0, 2.0, "junk"])).unwrap();

        assert_eq!(coerce(&value, GeometryKind::Point), Ok(None));
    }

    #[test]
    fn an_altitude_survives_a_three_element_coordinate_array() {
        let value = Value::try_from(json!([1.0, 2.0, 300.0])).unwrap();

        assert_eq!(
            coerce(&value, GeometryKind::Point),
            Ok(Some(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0, 300.0]),
            }))
        );
    }

    #[test]
    fn a_non_geometry_carries_no_geometry() {
        assert_eq!(coerce(&Value::try_from(json!("not a point")).unwrap(), GeometryKind::Point), Ok(None));
        assert_eq!(preserve(&Value::try_from(json!("not a point")).unwrap()), Ok(None));
        assert_eq!(preserve(&Value::Boolean(true)), Ok(None));
    }

    #[test]
    fn a_geojson_object_is_preserved_as_the_geometry_it_declares() {
        let value = Value::try_from(json!({"type": "Point", "coordinates": [1.0, 2.0]})).unwrap();

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
    fn only_an_already_typed_geometry_is_taken_as_one() {
        let point = NgsiLdGeometry::Point {
            coordinates: Position::from([1.0, 2.0]),
        };

        assert_eq!(Value::Geospatial(Box::new(point.clone())).into_geometry(), Some(point));
        assert_eq!(
            Value::try_from(json!({"type": "Point", "coordinates": [1.0, 2.0]})).unwrap().into_geometry(),
            None
        );
        assert_eq!(Value::String(r#"{"type":"Point","coordinates":[1.0,2.0]}"#.into()).into_geometry(), None);
        assert_eq!(Value::Boolean(true).into_geometry(), None);
    }

    #[test]
    fn a_source_geometry_collection_is_refused_rather_than_carried() {
        let value = Value::try_from(json!({"type": "GeometryCollection", "geometries": []})).unwrap();

        assert_eq!(preserve(&value), Err(GeometryError::GeometryCollection));
    }

    #[test]
    fn a_source_geometry_collection_folds_when_the_mapping_declares_it() {
        let value = Value::try_from(json!({
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [1.0, 2.0]},
                {"type": "Point", "coordinates": [3.0, 4.0]},
            ],
        }))
        .unwrap();
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
        let value = Value::try_from(json!({"type": "Point", "coordinates": [1.0, 2.0]})).unwrap();

        assert_eq!(
            coerce(&value, GeometryKind::MultiPoint),
            Ok(Some(NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0])],
            }))
        );
    }

    #[test]
    fn a_multipolygon_source_is_refused_toward_a_declared_polygon_without_a_conversion() {
        let value = Value::try_from(json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]],
            ],
        }))
        .unwrap();

        assert_eq!(
            coerce(&value, GeometryKind::Polygon),
            Err(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            })
        );
    }

    fn any(value: &Value, policy: GeometryPolicy) -> Result<Option<Value>, GeometryError> {
        value.to_any_geometry(GeometryTarget::Preserve, &policy)
    }

    /// The JSON the geometry `value` reads as writes, under the default policy.
    fn any_json(value: &Value) -> Result<Option<JsonValue>, GeometryError> {
        any(value, GeometryPolicy::default()).map(|read| read.map(JsonValue::from))
    }

    #[test]
    fn a_geometry_collection_reads_as_a_geojson_object() {
        let collection = json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]});

        // The JSON is consumed by the conversion into a value and still needed as the expected one.
        assert_eq!(any_json(&Value::try_from(collection.clone()).unwrap()), Ok(Some(collection)));
    }

    #[test]
    fn a_geometry_collection_holds_each_member_as_the_geometry_it_spells() {
        let collection = Value::try_from(json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]})).unwrap();

        assert_eq!(
            any(&collection, GeometryPolicy::default()),
            Ok(Some(Value::Object(Box::new(
                [
                    ("type".into(), Value::String("GeometryCollection".into())),
                    (
                        "geometries".into(),
                        Value::Array(vec![Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                            coordinates: [1.0, 2.0].into(),
                        }))])
                    ),
                ]
                .into_iter()
                .collect()
            ))))
        );
    }

    #[test]
    fn a_nested_geometry_collection_reads_as_the_geojson_object_it_is() {
        let collection = json!({
            "type": "GeometryCollection",
            "geometries": [{"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]}],
        });

        // The JSON is consumed by the conversion into a value and still needed as the expected one.
        assert_eq!(any_json(&Value::try_from(collection.clone()).unwrap()), Ok(Some(collection)));
    }

    #[test]
    fn a_geometry_collection_as_text_reads_as_a_geojson_object() {
        let text = Value::String(r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1.0,2.0]}]}"#.into());

        assert_eq!(
            any_json(&text),
            Ok(Some(
                json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]})
            ))
        );
    }

    #[test]
    fn a_geometry_collection_with_a_misshapen_member_is_no_geometry() {
        let collection = Value::try_from(json!({"type": "GeometryCollection", "geometries": [{"type": "Point"}]})).unwrap();

        assert_eq!(any(&collection, GeometryPolicy::default()), Ok(None));
    }

    #[test]
    fn a_geometry_collection_with_a_member_that_is_no_valid_geometry_is_refused() {
        let collection = Value::try_from(json!({
            "type": "GeometryCollection",
            "geometries": [{"type": "LineString", "coordinates": [[0.0, 0.0]]}],
        }))
        .unwrap();

        assert_eq!(
            any(&collection, GeometryPolicy::default()),
            Err(GeometryError::ShortLineString { positions: 1 })
        );
    }

    #[test]
    fn a_geometry_collection_under_a_declared_conversion_is_read_as_a_geo_property_reads_it() {
        let collection = Value::try_from(json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]})).unwrap();
        let flatten = GeometryPolicy::builder().convert(Some(ConversionStrategy::Flatten)).build();

        assert_eq!(
            any(&collection, flatten),
            Ok(Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [1.0, 2.0].into(),
            }))))
        );
        assert_eq!(
            collection.to_any_geometry(GeometryTarget::Coerce(GeometryKind::Point), &GeometryPolicy::default()),
            Err(GeometryError::GeometryCollection)
        );
    }

    #[test]
    fn any_geometry_reads_one_of_the_six_types_as_its_typed_geometry() {
        let point = Value::try_from(json!({"type": "Point", "coordinates": [16.37, 48.2]})).unwrap();

        assert_eq!(
            any(&point, GeometryPolicy::default()),
            Ok(Some(Value::Geospatial(Box::new(NgsiLdGeometry::Point {
                coordinates: [16.37, 48.2].into(),
            }))))
        );
    }
}

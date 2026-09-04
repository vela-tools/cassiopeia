use crate::{
    area_derivation,
    coordinates::CoordinateShape,
    error::GeometryError,
    geometry::{GeometryKind, NgsiLdGeometry},
    lattice::reconcile,
    line_derivation,
    normalisation::normalise,
    point_derivation,
    policy::GeometryPolicy,
    rfc7946::fold_collection,
    selection,
    strategy::ConversionStrategy,
    target::GeometryTarget,
    vertex_derivation,
};
use geojson::{Geometry, GeometryValue};

/// Admits a source `GeoJSON` geometry and converts it to the target under the mapping's policy.
///
/// # Errors
/// Returns the [`GeometryError`] naming why the source cannot become the target: a geometry type a
/// `GeoProperty` cannot hold, a loss the mapping did not authorise, or a result that breaks RFC 7946
/// clause 3.1's structural rules.
pub fn from_geojson(geometry: Geometry, target: GeometryTarget, policy: &GeometryPolicy) -> Result<NgsiLdGeometry, GeometryError> {
    let admitted = admit(geometry, *policy.convert())?;

    convert(admitted, target, policy)
}

/// Builds a geometry of `kind` out of raw source coordinates, then converts it to the target.
///
/// A bare coordinate array says nothing about which geometry type it means, so the mapping's
/// declared type supplies that and the array is read against it.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when the coordinates are not nested the way the type
/// requires, or the [`GeometryError`] naming the structural rule the result breaks.
pub fn from_coordinates(shape: CoordinateShape, kind: GeometryKind, policy: &GeometryPolicy) -> Result<NgsiLdGeometry, GeometryError> {
    let built = build(shape, kind)?;

    convert(built, GeometryTarget::Coerce(kind), policy)
}

/// Converts an admitted geometry to the target: the declared conversion first, then the lossless
/// reconciliation with the target type, then normalisation and the structural check.
///
/// # Errors
/// Returns the [`GeometryError`] naming the conversion that was refused or the structural rule the
/// result breaks.
pub fn convert(geometry: NgsiLdGeometry, target: GeometryTarget, policy: &GeometryPolicy) -> Result<NgsiLdGeometry, GeometryError> {
    let converted = match policy.convert() {
        Some(strategy) => apply(geometry, *strategy)?,
        None => geometry,
    };
    let mut reconciled = match target {
        GeometryTarget::Preserve => converted,
        GeometryTarget::Coerce(kind) => reconcile(converted, kind)?,
    };

    normalise(&mut reconciled, policy);
    reconciled.validate_structure()?;

    Ok(reconciled)
}

/// Admits one source geometry, folding a `GeometryCollection` only when the mapping declared the
/// conversion that folds one.
fn admit(geometry: Geometry, strategy: Option<ConversionStrategy>) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry.value {
        GeometryValue::GeometryCollection { geometries } => {
            if matches!(strategy, Some(ConversionStrategy::Flatten)) {
                fold_collection(geometries)
            } else {
                Err(GeometryError::GeometryCollection)
            }
        }
        value @ (GeometryValue::Point { .. }
        | GeometryValue::MultiPoint { .. }
        | GeometryValue::LineString { .. }
        | GeometryValue::MultiLineString { .. }
        | GeometryValue::Polygon { .. }
        | GeometryValue::MultiPolygon { .. }) => NgsiLdGeometry::try_from(value),
    }
}

/// Runs one declared conversion.
///
/// `flatten` is a no-op here: a collection was already folded when the source geometry was admitted,
/// and a source that was not a collection is simply kept.
fn apply(geometry: NgsiLdGeometry, strategy: ConversionStrategy) -> Result<NgsiLdGeometry, GeometryError> {
    match strategy {
        ConversionStrategy::First => selection::first(geometry),
        ConversionStrategy::Largest => selection::largest(geometry),
        ConversionStrategy::Centroid => point_derivation::centroid(&geometry),
        ConversionStrategy::PointOnSurface => point_derivation::point_on_surface(&geometry),
        ConversionStrategy::FirstVertex => point_derivation::first_vertex(&geometry),
        ConversionStrategy::BboxCenter => point_derivation::bbox_center(&geometry),
        ConversionStrategy::ExteriorRing => line_derivation::exterior_ring(geometry),
        ConversionStrategy::Boundary => line_derivation::boundary(geometry),
        ConversionStrategy::Connect => line_derivation::connect(geometry),
        ConversionStrategy::Ring => area_derivation::ring(geometry),
        ConversionStrategy::ConvexHull => area_derivation::convex_hull(&geometry),
        ConversionStrategy::Envelope => area_derivation::envelope(&geometry),
        ConversionStrategy::Vertices => vertex_derivation::vertices(geometry),
        ConversionStrategy::Flatten => Ok(geometry),
    }
}

/// Reads raw coordinates as the declared geometry type.
///
/// A polygon accepts either a bare ring or a list of rings, because a source that writes a single
/// ring as a flat list of positions is writing the common case of a polygon with no holes.
fn build(shape: CoordinateShape, kind: GeometryKind) -> Result<NgsiLdGeometry, GeometryError> {
    let unbuildable = GeometryError::Unbuildable { target: kind };
    let built = match kind {
        GeometryKind::Point => shape.into_position().map(|coordinates| NgsiLdGeometry::Point { coordinates }),
        GeometryKind::MultiPoint => shape
            .into_positions()
            .filter(|coordinates| !coordinates.is_empty())
            .map(|coordinates| NgsiLdGeometry::MultiPoint { coordinates }),
        GeometryKind::LineString => shape
            .into_positions()
            .filter(|coordinates| !coordinates.is_empty())
            .map(|coordinates| NgsiLdGeometry::LineString { coordinates }),
        GeometryKind::MultiLineString => shape
            .into_rings()
            .filter(|coordinates| !coordinates.is_empty())
            .map(|coordinates| NgsiLdGeometry::MultiLineString { coordinates }),
        GeometryKind::Polygon => build_polygon(shape),
        GeometryKind::MultiPolygon => shape
            .into_polygons()
            .filter(|coordinates| !coordinates.is_empty())
            .map(|coordinates| NgsiLdGeometry::MultiPolygon { coordinates }),
    };

    built.ok_or(unbuildable)
}

/// Reads raw coordinates as a polygon, accepting a bare exterior ring as well as a list of rings.
fn build_polygon(shape: CoordinateShape) -> Option<NgsiLdGeometry> {
    let coordinates = if shape.depth() > 1 {
        shape.into_rings()?
    } else {
        vec![shape.into_positions()?]
    };

    if coordinates.iter().any(Vec::is_empty) || coordinates.is_empty() {
        return None;
    }

    Some(NgsiLdGeometry::Polygon { coordinates })
}

#[cfg(test)]
mod tests {
    use crate::{
        convert::{convert, from_coordinates, from_geojson},
        coordinates::CoordinateShape,
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        policy::GeometryPolicy,
        strategy::ConversionStrategy,
        target::GeometryTarget,
    };
    use geojson::{Geometry, GeometryValue, Position};

    fn declaring(strategy: ConversionStrategy) -> GeometryPolicy {
        GeometryPolicy::builder().convert(Some(strategy)).build()
    }

    fn square(offset: f64, size: f64) -> Vec<Position> {
        vec![
            Position::from([offset, 0.0]),
            Position::from([offset + size, 0.0]),
            Position::from([offset + size, size]),
            Position::from([offset, size]),
            Position::from([offset, 0.0]),
        ]
    }

    fn multi_polygon() -> NgsiLdGeometry {
        NgsiLdGeometry::MultiPolygon {
            coordinates: vec![vec![square(0.0, 1.0)], vec![square(10.0, 3.0)]],
        }
    }

    #[test]
    fn a_point_promotes_to_a_multipoint_without_a_declaration() {
        let point = NgsiLdGeometry::Point {
            coordinates: Position::from([1.0, 2.0]),
        };

        assert_eq!(
            convert(point, GeometryTarget::Coerce(GeometryKind::MultiPoint), &GeometryPolicy::default()),
            Ok(NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0])],
            })
        );
    }

    #[test]
    fn a_multipolygon_of_several_members_is_refused_toward_a_polygon_without_a_declaration() {
        assert_eq!(
            convert(multi_polygon(), GeometryTarget::Coerce(GeometryKind::Polygon), &GeometryPolicy::default()),
            Err(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            })
        );
    }

    #[test]
    fn declaring_largest_demotes_a_multipolygon_to_its_biggest_surface() {
        assert_eq!(
            convert(
                multi_polygon(),
                GeometryTarget::Coerce(GeometryKind::Polygon),
                &declaring(ConversionStrategy::Largest)
            ),
            Ok(NgsiLdGeometry::Polygon {
                coordinates: vec![square(10.0, 3.0)],
            })
        );
    }

    #[test]
    fn declaring_a_point_on_surface_derives_a_point_from_a_multipolygon() {
        let derived = convert(
            multi_polygon(),
            GeometryTarget::Coerce(GeometryKind::Point),
            &declaring(ConversionStrategy::PointOnSurface),
        )
        .expect("a surface has an interior point");

        assert_eq!(derived.kind(), GeometryKind::Point);
    }

    #[test]
    fn a_source_geometry_collection_is_refused_unless_flatten_is_declared() {
        let collection = Geometry::new(GeometryValue::GeometryCollection {
            geometries: vec![
                Geometry::new(GeometryValue::Point {
                    coordinates: Position::from([1.0, 2.0]),
                }),
                Geometry::new(GeometryValue::Point {
                    coordinates: Position::from([3.0, 4.0]),
                }),
            ],
        });

        assert_eq!(
            from_geojson(collection.clone(), GeometryTarget::Preserve, &GeometryPolicy::default()),
            Err(GeometryError::GeometryCollection)
        );
        assert_eq!(
            from_geojson(collection, GeometryTarget::Preserve, &declaring(ConversionStrategy::Flatten)),
            Ok(NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0]), Position::from([3.0, 4.0])],
            })
        );
    }

    #[test]
    fn a_source_polygon_is_closed_and_rewound_on_the_way_in() {
        // A clockwise, unclosed exterior ring: malformed under RFC 7946 clause 3.1.6 on both counts.
        let source = Geometry::new(GeometryValue::Polygon {
            coordinates: vec![vec![
                Position::from([0.0, 0.0]),
                Position::from([0.0, 1.0]),
                Position::from([1.0, 1.0]),
                Position::from([1.0, 0.0]),
            ]],
        });

        assert_eq!(
            from_geojson(source, GeometryTarget::Preserve, &GeometryPolicy::default()),
            Ok(NgsiLdGeometry::Polygon {
                coordinates: vec![vec![
                    Position::from([0.0, 0.0]),
                    Position::from([1.0, 0.0]),
                    Position::from([1.0, 1.0]),
                    Position::from([0.0, 1.0]),
                    Position::from([0.0, 0.0]),
                ]],
            })
        );
    }

    #[test]
    fn a_bare_coordinate_pair_builds_the_declared_point() {
        let shape = CoordinateShape::Position(Position::from([14.5, 46.0]));

        assert_eq!(
            from_coordinates(shape, GeometryKind::Point, &GeometryPolicy::default()),
            Ok(NgsiLdGeometry::Point {
                coordinates: Position::from([14.5, 46.0]),
            })
        );
    }

    #[test]
    fn a_bare_ring_builds_a_polygon_and_is_closed() {
        let shape = CoordinateShape::Group(vec![
            CoordinateShape::Position(Position::from([0.0, 0.0])),
            CoordinateShape::Position(Position::from([1.0, 0.0])),
            CoordinateShape::Position(Position::from([1.0, 1.0])),
        ]);

        assert_eq!(
            from_coordinates(shape, GeometryKind::Polygon, &GeometryPolicy::default()),
            Ok(NgsiLdGeometry::Polygon {
                coordinates: vec![vec![
                    Position::from([0.0, 0.0]),
                    Position::from([1.0, 0.0]),
                    Position::from([1.0, 1.0]),
                    Position::from([0.0, 0.0]),
                ]],
            })
        );
    }

    #[test]
    fn a_list_too_short_to_close_into_a_ring_is_refused() {
        let shape = CoordinateShape::Group(vec![
            CoordinateShape::Position(Position::from([0.0, 0.0])),
            CoordinateShape::Position(Position::from([1.0, 0.0])),
        ]);

        assert_eq!(
            from_coordinates(shape, GeometryKind::Polygon, &GeometryPolicy::default()),
            Err(GeometryError::ShortRing { positions: 3 })
        );
    }

    #[test]
    fn a_single_position_is_no_curve() {
        let shape = CoordinateShape::Group(vec![CoordinateShape::Position(Position::from([0.0, 0.0]))]);

        assert_eq!(
            from_coordinates(shape, GeometryKind::LineString, &GeometryPolicy::default()),
            Err(GeometryError::ShortLineString { positions: 1 })
        );
    }

    #[test]
    fn coordinates_nested_the_wrong_way_for_the_declared_type_are_refused() {
        let shape = CoordinateShape::Position(Position::from([0.0, 0.0]));

        assert_eq!(
            from_coordinates(shape, GeometryKind::MultiPoint, &GeometryPolicy::default()),
            Err(GeometryError::Unbuildable {
                target: GeometryKind::MultiPoint,
            })
        );
    }
}

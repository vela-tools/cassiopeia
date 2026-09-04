use crate::{
    error::GeometryError,
    geometry::{GeometryKind, NgsiLdGeometry},
    measure::{line_length, polygon_area},
    strategy::ConversionStrategy,
};

/// Keeps the first member of a multi-geometry, in coordinate-array order.
///
/// The member's coordinates are taken verbatim, so an altitude (RFC 7946 clause 3.1.1) survives. A
/// geometry that already carries one member is its own first member.
///
/// # Errors
/// Returns [`GeometryError::EmptyGeometry`] when there is no member to take.
pub fn first(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry {
        single @ (NgsiLdGeometry::Point { .. } | NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::Polygon { .. }) => Ok(single),
        NgsiLdGeometry::MultiPoint { mut coordinates } => {
            if coordinates.is_empty() {
                return Err(GeometryError::EmptyGeometry);
            }
            Ok(NgsiLdGeometry::Point {
                coordinates: coordinates.swap_remove(0),
            })
        }
        NgsiLdGeometry::MultiLineString { mut coordinates } => {
            if coordinates.is_empty() {
                return Err(GeometryError::EmptyGeometry);
            }
            Ok(NgsiLdGeometry::LineString {
                coordinates: coordinates.swap_remove(0),
            })
        }
        NgsiLdGeometry::MultiPolygon { mut coordinates } => {
            if coordinates.is_empty() {
                return Err(GeometryError::EmptyGeometry);
            }
            Ok(NgsiLdGeometry::Polygon {
                coordinates: coordinates.swap_remove(0),
            })
        }
    }
}

/// Keeps the member of a multi-geometry with the greatest extent, its coordinates verbatim.
///
/// Extent is geodesic: the area a surface encloses, the length a curve runs. RFC 7946 clause 4 fixes
/// the coordinate reference system to WGS84 longitude/latitude, where a planar measurement would be
/// in square degrees and would rank members differently at different latitudes.
///
/// # Errors
/// Returns [`GeometryError::EmptyGeometry`] when there is no member to take, and
/// [`GeometryError::StrategyNotApplicable`] for a geometry of dimension zero, whose members have no
/// extent to be ranked by.
pub fn largest(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry {
        single @ (NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::Polygon { .. }) => Ok(single),
        NgsiLdGeometry::Point { .. } | NgsiLdGeometry::MultiPoint { .. } => Err(GeometryError::StrategyNotApplicable {
            strategy: ConversionStrategy::Largest,
            target: GeometryKind::Point,
        }),
        NgsiLdGeometry::MultiLineString { mut coordinates } => {
            let index = index_of_greatest(&coordinates, |line| line_length(line))?;
            Ok(NgsiLdGeometry::LineString {
                coordinates: coordinates.swap_remove(index),
            })
        }
        NgsiLdGeometry::MultiPolygon { mut coordinates } => {
            let index = index_of_greatest(&coordinates, |rings| polygon_area(rings))?;
            Ok(NgsiLdGeometry::Polygon {
                coordinates: coordinates.swap_remove(index),
            })
        }
    }
}

/// The index of the member measuring greatest, or a refusal when there are no members.
///
/// Ties keep the earlier member, so the choice is stable across runs over the same source.
fn index_of_greatest<T>(members: &[T], measure: impl Fn(&T) -> f64) -> Result<usize, GeometryError> {
    let mut greatest: Option<(usize, f64)> = None;
    for (index, member) in members.iter().enumerate() {
        let extent = measure(member);
        if greatest.is_none_or(|(_, best)| extent.total_cmp(&best).is_gt()) {
            greatest = Some((index, extent));
        }
    }

    greatest.map(|(index, _)| index).ok_or(GeometryError::EmptyGeometry)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::GeometryError,
        geometry::NgsiLdGeometry,
        selection::{first, largest},
    };
    use geojson::Position;

    /// A closed square ring of `size` degrees, anchored at the origin.
    fn square(size: f64) -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([size, 0.0]),
            Position::from([size, size]),
            Position::from([0.0, size]),
            Position::from([0.0, 0.0]),
        ]
    }

    #[test]
    fn first_takes_the_member_the_coordinate_array_lists_first() {
        let multi = NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([1.0, 2.0]), Position::from([3.0, 4.0])],
        };

        assert_eq!(
            first(multi),
            Ok(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0]),
            })
        );
    }

    #[test]
    fn first_keeps_an_altitude_verbatim() {
        let multi = NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([1.0, 2.0, 300.0])],
        };

        assert_eq!(
            first(multi),
            Ok(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0, 300.0]),
            })
        );
    }

    #[test]
    fn largest_takes_the_member_enclosing_the_greatest_area() {
        let multi = NgsiLdGeometry::MultiPolygon {
            coordinates: vec![vec![square(1.0)], vec![square(3.0)], vec![square(2.0)]],
        };

        assert_eq!(
            largest(multi),
            Ok(NgsiLdGeometry::Polygon {
                coordinates: vec![square(3.0)]
            })
        );
    }

    #[test]
    fn largest_takes_the_longest_curve() {
        let multi = NgsiLdGeometry::MultiLineString {
            coordinates: vec![
                vec![Position::from([0.0, 0.0]), Position::from([1.0, 0.0])],
                vec![Position::from([0.0, 0.0]), Position::from([5.0, 0.0])],
            ],
        };

        assert_eq!(
            largest(multi),
            Ok(NgsiLdGeometry::LineString {
                coordinates: vec![Position::from([0.0, 0.0]), Position::from([5.0, 0.0])],
            })
        );
    }

    #[test]
    fn largest_has_no_ordering_to_apply_to_positions() {
        let multi = NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([1.0, 2.0])],
        };

        assert!(matches!(largest(multi), Err(GeometryError::StrategyNotApplicable { .. })));
    }

    #[test]
    fn selecting_from_an_empty_multi_geometry_fabricates_nothing() {
        assert_eq!(
            first(NgsiLdGeometry::MultiPolygon { coordinates: Vec::new() }),
            Err(GeometryError::EmptyGeometry)
        );
        assert_eq!(
            largest(NgsiLdGeometry::MultiLineString { coordinates: Vec::new() }),
            Err(GeometryError::EmptyGeometry)
        );
    }
}

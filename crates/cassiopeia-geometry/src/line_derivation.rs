use crate::{
    error::GeometryError,
    geometry::{GeometryKind, NgsiLdGeometry},
};

/// Takes a surface's exterior ring as a closed curve, discarding its holes.
///
/// The ring's positions are moved verbatim, so an altitude survives; what is lost is the holes, and
/// with them the surface interpretation. A `MultiPolygon` contributes one curve per surface.
///
/// # Errors
/// Returns [`GeometryError::Uncoercible`] for a geometry that bounds no surface and
/// [`GeometryError::EmptyGeometry`] when there is no ring to take.
pub fn exterior_ring(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry {
        NgsiLdGeometry::Polygon { coordinates } => coordinates
            .into_iter()
            .next()
            .map(|exterior| NgsiLdGeometry::LineString { coordinates: exterior })
            .ok_or(GeometryError::EmptyGeometry),
        NgsiLdGeometry::MultiPolygon { coordinates } => {
            let exteriors: Vec<_> = coordinates.into_iter().filter_map(|rings| rings.into_iter().next()).collect();
            if exteriors.is_empty() {
                return Err(GeometryError::EmptyGeometry);
            }
            Ok(NgsiLdGeometry::MultiLineString { coordinates: exteriors })
        }
        other @ (NgsiLdGeometry::Point { .. }
        | NgsiLdGeometry::MultiPoint { .. }
        | NgsiLdGeometry::LineString { .. }
        | NgsiLdGeometry::MultiLineString { .. }) => Err(GeometryError::Uncoercible {
            origin: other.kind(),
            target: GeometryKind::LineString,
        }),
    }
}

/// Takes a surface's whole boundary as one curve per ring, exterior first.
///
/// Every coordinate survives (this is a regrouping, not a derivation), and only the reading of
/// those rings as a bounded surface is dropped.
///
/// # Errors
/// Returns [`GeometryError::Uncoercible`] for a geometry that bounds no surface and
/// [`GeometryError::EmptyGeometry`] when there is no ring to take.
pub fn boundary(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    let rings: Vec<_> = match geometry {
        NgsiLdGeometry::Polygon { coordinates } => coordinates,
        NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.into_iter().flatten().collect(),
        other @ (NgsiLdGeometry::Point { .. }
        | NgsiLdGeometry::MultiPoint { .. }
        | NgsiLdGeometry::LineString { .. }
        | NgsiLdGeometry::MultiLineString { .. }) => {
            return Err(GeometryError::Uncoercible {
                origin: other.kind(),
                target: GeometryKind::MultiLineString,
            });
        }
    };

    if rings.is_empty() {
        return Err(GeometryError::EmptyGeometry);
    }

    Ok(NgsiLdGeometry::MultiLineString { coordinates: rings })
}

/// Joins a `MultiPoint`'s positions into one curve, in coordinate-array order.
///
/// The order is the source's, not a route: the conversion draws the polyline the coordinate array
/// already implies and invents no other ordering.
///
/// # Errors
/// Returns [`GeometryError::Uncoercible`] for a geometry that is not a set of positions, and
/// [`GeometryError::ShortLineString`] when there are too few positions for a curve (RFC 7946
/// clause 3.1.4).
pub fn connect(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry {
        NgsiLdGeometry::MultiPoint { coordinates } => {
            if coordinates.len() < 2 {
                return Err(GeometryError::ShortLineString { positions: coordinates.len() });
            }
            Ok(NgsiLdGeometry::LineString { coordinates })
        }
        other @ (NgsiLdGeometry::Point { .. }
        | NgsiLdGeometry::LineString { .. }
        | NgsiLdGeometry::MultiLineString { .. }
        | NgsiLdGeometry::Polygon { .. }
        | NgsiLdGeometry::MultiPolygon { .. }) => Err(GeometryError::Uncoercible {
            origin: other.kind(),
            target: GeometryKind::LineString,
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        line_derivation::{boundary, connect, exterior_ring},
    };
    use geojson::Position;

    fn exterior() -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([3.0, 0.0]),
            Position::from([3.0, 3.0]),
            Position::from([0.0, 0.0]),
        ]
    }

    fn hole() -> Vec<Position> {
        vec![
            Position::from([1.0, 1.0]),
            Position::from([2.0, 1.0]),
            Position::from([2.0, 2.0]),
            Position::from([1.0, 1.0]),
        ]
    }

    #[test]
    fn the_exterior_ring_of_a_polygon_with_a_hole_keeps_only_the_outer_curve() {
        let polygon = NgsiLdGeometry::Polygon {
            coordinates: vec![exterior(), hole()],
        };

        assert_eq!(exterior_ring(polygon), Ok(NgsiLdGeometry::LineString { coordinates: exterior() }));
    }

    #[test]
    fn the_boundary_of_a_polygon_with_a_hole_keeps_every_ring() {
        let polygon = NgsiLdGeometry::Polygon {
            coordinates: vec![exterior(), hole()],
        };

        assert_eq!(
            boundary(polygon),
            Ok(NgsiLdGeometry::MultiLineString {
                coordinates: vec![exterior(), hole()],
            })
        );
    }

    #[test]
    fn a_curve_has_no_boundary_to_take() {
        let line = NgsiLdGeometry::LineString {
            coordinates: vec![Position::from([0.0, 0.0]), Position::from([1.0, 1.0])],
        };

        assert_eq!(
            boundary(line),
            Err(GeometryError::Uncoercible {
                origin: GeometryKind::LineString,
                target: GeometryKind::MultiLineString,
            })
        );
    }

    #[test]
    fn connecting_positions_keeps_their_coordinate_array_order() {
        let multi = NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([0.0, 0.0]), Position::from([1.0, 1.0])],
        };

        assert_eq!(
            connect(multi),
            Ok(NgsiLdGeometry::LineString {
                coordinates: vec![Position::from([0.0, 0.0]), Position::from([1.0, 1.0])],
            })
        );
    }

    #[test]
    fn connecting_a_single_position_draws_no_curve() {
        let multi = NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([0.0, 0.0])],
        };

        assert_eq!(connect(multi), Err(GeometryError::ShortLineString { positions: 1 }));
    }
}

use crate::{
    error::GeometryError,
    geometry::{GeometryKind, NgsiLdGeometry},
    planar::{from_polygon, from_rect, to_planar},
    ring::require_ring,
};
use geo::{BoundingRect, ConvexHull};

/// Reads a closed curve as a surface, the curve becoming the polygon's exterior ring.
///
/// The curve must already close and hold at least four positions (RFC 7946 clause 3.1.6). An open
/// curve is refused rather than closed: appending the first position would draw a segment the source
/// never drew, and whether that segment is wanted is the mapping author's call, not this crate's.
///
/// A `MultiLineString` becomes one single-ring surface per curve.
///
/// # Errors
/// Returns [`GeometryError::Uncoercible`] for a geometry that is not a curve,
/// [`GeometryError::ShortRing`] or [`GeometryError::UnclosedRing`] for a curve that is no ring, and
/// [`GeometryError::EmptyGeometry`] when there is no curve to read.
pub fn ring(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry {
        NgsiLdGeometry::LineString { coordinates } => {
            require_ring(&coordinates)?;
            Ok(NgsiLdGeometry::Polygon {
                coordinates: vec![coordinates],
            })
        }
        NgsiLdGeometry::MultiLineString { coordinates } => {
            if coordinates.is_empty() {
                return Err(GeometryError::EmptyGeometry);
            }
            for curve in &coordinates {
                require_ring(curve)?;
            }
            Ok(NgsiLdGeometry::MultiPolygon {
                coordinates: coordinates.into_iter().map(|curve| vec![curve]).collect(),
            })
        }
        other @ (NgsiLdGeometry::Point { .. } | NgsiLdGeometry::MultiPoint { .. } | NgsiLdGeometry::Polygon { .. } | NgsiLdGeometry::MultiPolygon { .. }) => {
            Err(GeometryError::Uncoercible {
                origin: other.kind(),
                target: GeometryKind::Polygon,
            })
        }
    }
}

/// Takes the convex hull of every position the geometry lists.
///
/// The hull is computed in the plane and rebuilt from computed coordinates, so an altitude does not
/// survive it.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when a position cannot be read; a hull too degenerate to
/// bound a surface is refused by the structural check the conversion ends with.
pub fn convex_hull(geometry: &NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    Ok(from_polygon(&to_planar(geometry)?.convex_hull()))
}

/// Takes the geometry's bounding box as a rectangular surface.
///
/// Available from every source, including a lone position, whose bounding box is degenerate and is
/// refused by the structural check the conversion ends with rather than being emitted as a
/// zero-area rectangle.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when a position cannot be read and
/// [`GeometryError::EmptyGeometry`] when the geometry has no extent to bound.
pub fn envelope(geometry: &NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    let rect = to_planar(geometry)?.bounding_rect().ok_or(GeometryError::EmptyGeometry)?;

    Ok(from_rect(&rect))
}

#[cfg(test)]
mod tests {
    use crate::{
        area_derivation::{convex_hull, envelope, ring},
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
    };
    use geojson::Position;

    fn closed_curve() -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([1.0, 0.0]),
            Position::from([1.0, 1.0]),
            Position::from([0.0, 0.0]),
        ]
    }

    /// The set of distinct `[x, y]` pairs a geometry's exterior ring lists.
    fn exterior(geometry: &NgsiLdGeometry) -> Vec<Vec<f64>> {
        match geometry {
            NgsiLdGeometry::Polygon { coordinates } => coordinates
                .first()
                .map(|ring| ring.iter().map(|position| position.as_slice().to_vec()).collect())
                .unwrap_or_default(),
            NgsiLdGeometry::Point { .. }
            | NgsiLdGeometry::MultiPoint { .. }
            | NgsiLdGeometry::LineString { .. }
            | NgsiLdGeometry::MultiLineString { .. }
            | NgsiLdGeometry::MultiPolygon { .. } => Vec::new(),
        }
    }

    #[test]
    fn a_closed_curve_reads_as_a_single_ring_surface() {
        let curve = NgsiLdGeometry::LineString { coordinates: closed_curve() };

        assert_eq!(
            ring(curve),
            Ok(NgsiLdGeometry::Polygon {
                coordinates: vec![closed_curve()],
            })
        );
    }

    #[test]
    fn an_open_curve_is_refused_rather_than_closed() {
        let curve = NgsiLdGeometry::LineString {
            coordinates: vec![
                Position::from([0.0, 0.0]),
                Position::from([1.0, 0.0]),
                Position::from([1.0, 1.0]),
                Position::from([0.0, 1.0]),
            ],
        };

        assert_eq!(ring(curve), Err(GeometryError::UnclosedRing));
    }

    #[test]
    fn a_surface_has_no_curve_to_read_as_a_ring() {
        let polygon = NgsiLdGeometry::Polygon {
            coordinates: vec![closed_curve()],
        };

        assert_eq!(
            ring(polygon),
            Err(GeometryError::Uncoercible {
                origin: GeometryKind::Polygon,
                target: GeometryKind::Polygon,
            })
        );
    }

    #[test]
    fn the_convex_hull_of_a_set_of_positions_bounds_them_all() {
        let cloud = NgsiLdGeometry::MultiPoint {
            coordinates: vec![
                Position::from([0.0, 0.0]),
                Position::from([2.0, 0.0]),
                Position::from([2.0, 2.0]),
                Position::from([0.0, 2.0]),
                Position::from([1.0, 1.0]),
            ],
        };
        let hull = convex_hull(&cloud).expect("a point cloud has a hull");

        // The interior position is not a hull vertex, so the closed ring holds the four corners.
        assert_eq!(exterior(&hull).len(), 5);
    }

    #[test]
    fn the_envelope_of_a_curve_is_its_bounding_rectangle() {
        let curve = NgsiLdGeometry::LineString {
            coordinates: vec![Position::from([1.0, 2.0]), Position::from([4.0, 6.0])],
        };
        let box_polygon = envelope(&curve).expect("a curve has a bounding box");
        let corners = exterior(&box_polygon);

        assert_eq!(corners.len(), 5);
        assert!(corners.contains(&vec![1.0, 2.0]));
        assert!(corners.contains(&vec![4.0, 6.0]));
        assert!(corners.contains(&vec![1.0, 6.0]));
        assert!(corners.contains(&vec![4.0, 2.0]));
    }
}

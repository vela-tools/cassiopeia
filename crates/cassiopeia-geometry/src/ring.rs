use crate::{error::GeometryError, geometry::NgsiLdGeometry, measure::signed_ring_area};
use geojson::{LineStringType, PolygonType, Position};

/// Which role a linear ring plays inside a polygon.
///
/// RFC 7946 clause 3.1.6 states the right-hand rule as a producer requirement, and the rule reads
/// differently for the two roles: an exterior ring runs counterclockwise, a hole clockwise.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RingRole {
    /// The ring bounding the surface, wound counterclockwise.
    Exterior,
    /// A ring bounding a hole in the surface, wound clockwise.
    Interior,
}

/// Whether a ring's last position repeats its first, as RFC 7946 clause 3.1.6 requires.
///
/// The clause asks for identical values, so an altitude carried on one end and not the other leaves
/// the ring open.
#[must_use]
pub fn is_closed(ring: &[Position]) -> bool {
    match (ring.first(), ring.last()) {
        (Some(first), Some(last)) => first == last,
        (None, _) | (_, None) => false,
    }
}

/// Checks that a list of positions is a linear ring: at least four positions, the last repeating
/// the first (RFC 7946 clause 3.1.6).
///
/// # Errors
/// Returns [`GeometryError::ShortRing`] when the ring is too short and
/// [`GeometryError::UnclosedRing`] when it does not close. A ring is never closed on the source's
/// behalf: silently appending the first position would invent a segment the source never drew.
pub fn require_ring(ring: &[Position]) -> Result<(), GeometryError> {
    if ring.len() < 4 {
        return Err(GeometryError::ShortRing { positions: ring.len() });
    }
    if !is_closed(ring) {
        return Err(GeometryError::UnclosedRing);
    }

    Ok(())
}

/// Closes a ring that does not already end where it starts, by repeating its first position.
///
/// A polygon ring is a *linear* ring by definition (RFC 7946 clause 3.1.6), so the closing segment
/// is implied by the source having written a polygon at all and repeating the position only makes
/// it explicit. A curve is a different matter: reading one as a ring is what the `ring` conversion
/// does, and there the closing segment is refused rather than invented.
pub fn close(ring: &mut LineStringType) {
    if ring.is_empty() || is_closed(ring) {
        return;
    }

    let first = ring[0].clone();
    ring.push(first);
}

/// Closes every ring of one polygon.
pub fn close_polygon(rings: &mut PolygonType) {
    for ring in rings.iter_mut() {
        close(ring);
    }
}

/// Closes every ring the geometry carries; geometries below dimension two carry none.
pub fn close_rings(geometry: &mut NgsiLdGeometry) {
    match geometry {
        NgsiLdGeometry::Point { .. } | NgsiLdGeometry::MultiPoint { .. } | NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::MultiLineString { .. } => {}
        NgsiLdGeometry::Polygon { coordinates } => close_polygon(coordinates),
        NgsiLdGeometry::MultiPolygon { coordinates } => {
            for rings in coordinates.iter_mut() {
                close_polygon(rings);
            }
        }
    }
}

/// Rewinds one ring to the direction its role requires, reversing it in place.
///
/// Reversing the positions rather than rebuilding the ring from a planar model is what keeps an
/// altitude (RFC 7946 clause 3.1.1) attached to the position that carried it.
pub fn rewind(ring: &mut LineStringType, role: RingRole) {
    let area = signed_ring_area(ring);
    let reversed = match role {
        RingRole::Exterior => area < 0.0,
        RingRole::Interior => area > 0.0,
    };

    if reversed {
        ring.reverse();
    }
}

/// Rewinds every ring of one polygon: the exterior counterclockwise, each hole clockwise.
pub fn rewind_polygon(rings: &mut PolygonType) {
    let Some((exterior, interiors)) = rings.split_first_mut() else {
        return;
    };

    rewind(exterior, RingRole::Exterior);
    for interior in interiors {
        rewind(interior, RingRole::Interior);
    }
}

/// Rewinds every ring the geometry carries to RFC 7946 clause 3.1.6's right-hand rule.
///
/// Geometries below dimension two carry no ring and are left untouched.
pub fn normalise_winding(geometry: &mut NgsiLdGeometry) {
    match geometry {
        NgsiLdGeometry::Point { .. } | NgsiLdGeometry::MultiPoint { .. } | NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::MultiLineString { .. } => {}
        NgsiLdGeometry::Polygon { coordinates } => rewind_polygon(coordinates),
        NgsiLdGeometry::MultiPolygon { coordinates } => {
            for rings in coordinates.iter_mut() {
                rewind_polygon(rings);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::GeometryError,
        geometry::NgsiLdGeometry,
        ring::{close_rings, is_closed, normalise_winding, require_ring},
    };
    use geojson::Position;

    /// A one-degree square wound clockwise, the way ESRI shapefiles write an exterior ring.
    fn clockwise_ring() -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([0.0, 1.0]),
            Position::from([1.0, 1.0]),
            Position::from([1.0, 0.0]),
            Position::from([0.0, 0.0]),
        ]
    }

    #[test]
    fn a_clockwise_exterior_ring_is_rewound_counterclockwise() {
        let mut geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![clockwise_ring()],
        };
        normalise_winding(&mut geometry);

        let mut expected = clockwise_ring();
        expected.reverse();
        assert_eq!(geometry, NgsiLdGeometry::Polygon { coordinates: vec![expected] });
    }

    #[test]
    fn a_hole_is_rewound_clockwise_while_the_exterior_stays_counterclockwise() {
        let mut exterior = clockwise_ring();
        exterior.reverse();
        let hole = vec![
            Position::from([0.2, 0.2]),
            Position::from([0.8, 0.2]),
            Position::from([0.8, 0.8]),
            Position::from([0.2, 0.2]),
        ];
        let mut geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![exterior.clone(), hole.clone()],
        };
        normalise_winding(&mut geometry);

        let mut wound_hole = hole;
        wound_hole.reverse();
        assert_eq!(
            geometry,
            NgsiLdGeometry::Polygon {
                coordinates: vec![exterior, wound_hole],
            }
        );
    }

    #[test]
    fn rewinding_keeps_the_altitude_attached_to_the_position_that_carried_it() {
        let ring = vec![
            Position::from([0.0, 0.0, 10.0]),
            Position::from([0.0, 1.0, 20.0]),
            Position::from([1.0, 1.0, 30.0]),
            Position::from([1.0, 0.0, 40.0]),
            Position::from([0.0, 0.0, 10.0]),
        ];
        let mut geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![ring.clone()],
        };
        normalise_winding(&mut geometry);

        let mut expected = ring;
        expected.reverse();
        assert_eq!(geometry, NgsiLdGeometry::Polygon { coordinates: vec![expected] });
    }

    #[test]
    fn a_ring_shorter_than_four_positions_is_refused() {
        let ring = vec![Position::from([0.0, 0.0]), Position::from([1.0, 0.0]), Position::from([0.0, 0.0])];

        assert_eq!(require_ring(&ring), Err(GeometryError::ShortRing { positions: 3 }));
    }

    #[test]
    fn an_unclosed_polygon_ring_is_closed_by_repeating_its_first_position() {
        let mut geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                Position::from([0.0, 0.0]),
                Position::from([1.0, 0.0]),
                Position::from([1.0, 1.0]),
                Position::from([0.0, 1.0]),
            ]],
        };
        close_rings(&mut geometry);

        let NgsiLdGeometry::Polygon { coordinates } = &geometry else {
            panic!("still a polygon");
        };
        assert_eq!(coordinates[0].len(), 5);
        assert!(is_closed(&coordinates[0]));
    }

    #[test]
    fn an_unclosed_ring_is_refused_rather_than_closed() {
        let ring = vec![
            Position::from([0.0, 0.0]),
            Position::from([1.0, 0.0]),
            Position::from([1.0, 1.0]),
            Position::from([0.0, 1.0]),
        ];

        assert!(!is_closed(&ring));
        assert_eq!(require_ring(&ring), Err(GeometryError::UnclosedRing));
    }
}

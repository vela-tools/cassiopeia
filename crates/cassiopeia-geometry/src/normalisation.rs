use crate::{
    geometry::NgsiLdGeometry,
    policy::GeometryPolicy,
    ring::{close_rings, normalise_winding},
    strategy::{Altitude, Winding},
};
use geojson::Position;

/// Brings a geometry into the producer form RFC 7946 asks for, under the mapping's policy.
///
/// Three passes, in order: an altitude is discarded when the mapping asked for that, every polygon
/// ring is closed (clause 3.1.6 defines a polygon over *linear* rings, so a source that left one
/// open wrote a malformed polygon rather than a different shape), and every ring is rewound to the
/// right-hand rule unless the mapping asked to keep the source's winding.
pub fn normalise(geometry: &mut NgsiLdGeometry, policy: &GeometryPolicy) {
    match policy.altitude() {
        Altitude::Keep => {}
        Altitude::Drop => drop_altitude(geometry),
    }

    close_rings(geometry);

    match policy.winding() {
        Winding::Rfc7946 => normalise_winding(geometry),
        Winding::Keep => {}
    }
}

/// Truncates every position to a longitude and a latitude.
fn drop_altitude(geometry: &mut NgsiLdGeometry) {
    match geometry {
        NgsiLdGeometry::Point { coordinates } => truncate(coordinates),
        NgsiLdGeometry::MultiPoint { coordinates } | NgsiLdGeometry::LineString { coordinates } => coordinates.iter_mut().for_each(truncate),
        NgsiLdGeometry::MultiLineString { coordinates } | NgsiLdGeometry::Polygon { coordinates } => {
            coordinates.iter_mut().flatten().for_each(truncate);
        }
        NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.iter_mut().flatten().flatten().for_each(truncate),
    }
}

/// Truncates one position, leaving a position too short to truncate alone so the structural check
/// reports it rather than this pass hiding it.
fn truncate(position: &mut Position) {
    if position.len() > 2 {
        *position = Position::from([position[0], position[1]]);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::NgsiLdGeometry,
        normalisation::normalise,
        policy::GeometryPolicy,
        strategy::{Altitude, Winding},
    };
    use geojson::Position;

    /// A one-degree square wound clockwise, as an ESRI shapefile writes an exterior ring.
    fn clockwise() -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([0.0, 1.0]),
            Position::from([1.0, 1.0]),
            Position::from([1.0, 0.0]),
            Position::from([0.0, 0.0]),
        ]
    }

    #[test]
    fn the_default_policy_rewinds_a_clockwise_exterior_ring() {
        let mut geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![clockwise()],
        };
        normalise(&mut geometry, &GeometryPolicy::default());

        let mut expected = clockwise();
        expected.reverse();
        assert_eq!(geometry, NgsiLdGeometry::Polygon { coordinates: vec![expected] });
    }

    #[test]
    fn keeping_the_winding_leaves_the_source_ring_alone() {
        let mut geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![clockwise()],
        };
        let policy = GeometryPolicy::builder().winding(Winding::Keep).build();
        normalise(&mut geometry, &policy);

        assert_eq!(
            geometry,
            NgsiLdGeometry::Polygon {
                coordinates: vec![clockwise()]
            }
        );
    }

    #[test]
    fn dropping_the_altitude_truncates_every_position() {
        let mut geometry = NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([1.0, 2.0, 300.0]), Position::from([3.0, 4.0])],
        };
        let policy = GeometryPolicy::builder().altitude(Altitude::Drop).build();
        normalise(&mut geometry, &policy);

        assert_eq!(
            geometry,
            NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0]), Position::from([3.0, 4.0])],
            }
        );
    }

    #[test]
    fn the_default_policy_keeps_an_altitude() {
        let mut geometry = NgsiLdGeometry::Point {
            coordinates: Position::from([1.0, 2.0, 300.0]),
        };
        normalise(&mut geometry, &GeometryPolicy::default());

        assert_eq!(
            geometry,
            NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0, 300.0]),
            }
        );
    }
}

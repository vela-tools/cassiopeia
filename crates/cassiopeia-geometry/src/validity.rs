use crate::{error::GeometryError, geometry::NgsiLdGeometry, ring::require_ring};
use geojson::{LineStringType, PolygonType, Position};

impl NgsiLdGeometry {
    /// Checks the structural rules RFC 7946 clause 3.1 states but the type itself cannot encode:
    /// how many components a position carries, how many positions a curve needs, and whether a
    /// linear ring is long enough and closed.
    ///
    /// An empty coordinates array is accepted rather than refused: clause 3.1 permits one and lets
    /// a processor read it as a null object. The builders refuse to *produce* one, and demoting an
    /// empty multi-geometry is refused rather than fabricated, so an empty geometry only ever
    /// arrives from a source that wrote one.
    ///
    /// This is structural validity only. It does not establish OGC simple-feature validity: a ring
    /// that crosses itself passes here and is still topologically bogus.
    ///
    /// # Errors
    /// Returns the [`GeometryError`] naming the first rule the geometry breaks.
    pub fn validate_structure(&self) -> Result<(), GeometryError> {
        match self {
            NgsiLdGeometry::Point { coordinates } => check_position(coordinates),
            NgsiLdGeometry::MultiPoint { coordinates } => coordinates.iter().try_for_each(check_position),
            NgsiLdGeometry::LineString { coordinates } => check_line(coordinates),
            NgsiLdGeometry::MultiLineString { coordinates } => coordinates.iter().try_for_each(|line| check_line(line)),
            NgsiLdGeometry::Polygon { coordinates } => check_rings(coordinates),
            NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.iter().try_for_each(check_rings),
        }
    }
}

/// Checks that a position carries at least a longitude and a latitude (RFC 7946 clause 3.1.1).
fn check_position(position: &Position) -> Result<(), GeometryError> {
    if position.len() < 2 {
        return Err(GeometryError::ShortPosition { components: position.len() });
    }

    Ok(())
}

/// Checks that a curve holds at least two positions (RFC 7946 clause 3.1.4), or none at all.
fn check_line(positions: &[Position]) -> Result<(), GeometryError> {
    if positions.is_empty() {
        return Ok(());
    }
    if positions.len() < 2 {
        return Err(GeometryError::ShortLineString { positions: positions.len() });
    }

    positions.iter().try_for_each(check_position)
}

/// Checks every ring of one polygon (RFC 7946 clause 3.1.6).
fn check_rings(rings: &PolygonType) -> Result<(), GeometryError> {
    rings.iter().try_for_each(check_ring)
}

/// Checks one linear ring: long enough, closed, and made of readable positions.
fn check_ring(ring: &LineStringType) -> Result<(), GeometryError> {
    require_ring(ring)?;

    ring.iter().try_for_each(check_position)
}

#[cfg(test)]
mod tests {
    use crate::{error::GeometryError, geometry::NgsiLdGeometry};
    use geojson::Position;

    #[test]
    fn a_well_formed_geometry_of_each_kind_passes() {
        let ring = vec![
            Position::from([0.0, 0.0]),
            Position::from([1.0, 0.0]),
            Position::from([1.0, 1.0]),
            Position::from([0.0, 0.0]),
        ];
        for geometry in [
            NgsiLdGeometry::Point {
                coordinates: [1.0, 2.0].into(),
            },
            NgsiLdGeometry::MultiPoint {
                coordinates: vec![[1.0, 2.0].into()],
            },
            NgsiLdGeometry::LineString {
                coordinates: vec![[0.0, 0.0].into(), [1.0, 1.0].into()],
            },
            NgsiLdGeometry::MultiLineString {
                coordinates: vec![vec![[0.0, 0.0].into(), [1.0, 1.0].into()]],
            },
            NgsiLdGeometry::Polygon {
                coordinates: vec![ring.clone()],
            },
            NgsiLdGeometry::MultiPolygon { coordinates: vec![vec![ring]] },
        ] {
            assert_eq!(geometry.validate_structure(), Ok(()), "rejected {geometry}");
        }
    }

    #[test]
    fn a_position_with_one_component_is_rejected() {
        let geometry = NgsiLdGeometry::Point {
            coordinates: Position::from(vec![1.0]),
        };

        assert_eq!(geometry.validate_structure(), Err(GeometryError::ShortPosition { components: 1 }));
    }

    #[test]
    fn a_line_of_one_position_is_rejected() {
        let geometry = NgsiLdGeometry::LineString {
            coordinates: vec![[1.0, 2.0].into()],
        };

        assert_eq!(geometry.validate_structure(), Err(GeometryError::ShortLineString { positions: 1 }));
    }

    #[test]
    fn a_polygon_whose_ring_is_open_is_rejected() {
        let geometry = NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                Position::from([0.0, 0.0]),
                Position::from([1.0, 0.0]),
                Position::from([1.0, 1.0]),
                Position::from([0.0, 1.0]),
            ]],
        };

        assert_eq!(geometry.validate_structure(), Err(GeometryError::UnclosedRing));
    }

    #[test]
    fn an_empty_multi_geometry_is_accepted_as_the_null_object_the_rfc_permits() {
        assert_eq!(NgsiLdGeometry::MultiPoint { coordinates: Vec::new() }.validate_structure(), Ok(()));
        assert_eq!(NgsiLdGeometry::Polygon { coordinates: Vec::new() }.validate_structure(), Ok(()));
    }
}

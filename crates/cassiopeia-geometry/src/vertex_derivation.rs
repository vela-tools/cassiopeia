use crate::{error::GeometryError, geometry::NgsiLdGeometry};
use geojson::Position;
use std::collections::HashSet;

/// Takes every distinct position the geometry lists, in coordinate-array order, as a `MultiPoint`.
///
/// A polygon contributes the positions of all its rings, holes included, not only of its exterior
/// one: the conversion is about the vertices the source drew, not about the surface they bound. The
/// positions are moved rather than recomputed, so an altitude (RFC 7946 clause 3.1.1) survives.
///
/// Two positions count as the same vertex when their coordinates are bit-for-bit identical, which
/// is what a repeated vertex (a ring's closing position, a shared boundary) actually is in a
/// source document.
///
/// # Errors
/// Returns [`GeometryError::EmptyGeometry`] when the geometry lists no position at all.
pub fn vertices(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    let mut seen: HashSet<Vec<u64>> = HashSet::new();
    let mut kept: Vec<Position> = Vec::new();
    {
        let mut keep = |position: Position| {
            if seen.insert(fingerprint(&position)) {
                kept.push(position);
            }
        };

        match geometry {
            NgsiLdGeometry::Point { coordinates } => keep(coordinates),
            NgsiLdGeometry::MultiPoint { coordinates } | NgsiLdGeometry::LineString { coordinates } => coordinates.into_iter().for_each(&mut keep),
            NgsiLdGeometry::MultiLineString { coordinates } | NgsiLdGeometry::Polygon { coordinates } => {
                coordinates.into_iter().flatten().for_each(&mut keep);
            }
            NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.into_iter().flatten().flatten().for_each(&mut keep),
        }
    }

    if kept.is_empty() {
        return Err(GeometryError::EmptyGeometry);
    }

    Ok(NgsiLdGeometry::MultiPoint { coordinates: kept })
}

/// The first position the geometry lists, in coordinate-array order.
#[must_use]
pub fn first_position(geometry: &NgsiLdGeometry) -> Option<&Position> {
    match geometry {
        NgsiLdGeometry::Point { coordinates } => Some(coordinates),
        NgsiLdGeometry::MultiPoint { coordinates } | NgsiLdGeometry::LineString { coordinates } => coordinates.first(),
        NgsiLdGeometry::MultiLineString { coordinates } | NgsiLdGeometry::Polygon { coordinates } => coordinates.first()?.first(),
        NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.first()?.first()?.first(),
    }
}

/// The exact coordinate values of a position, as the key two identical vertices share.
fn fingerprint(position: &Position) -> Vec<u64> {
    position.as_slice().iter().map(|component| component.to_bits()).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        error::GeometryError,
        geometry::NgsiLdGeometry,
        vertex_derivation::{first_position, vertices},
    };
    use geojson::Position;

    fn ring() -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([1.0, 0.0]),
            Position::from([1.0, 1.0]),
            Position::from([0.0, 0.0]),
        ]
    }

    #[test]
    fn a_polygon_contributes_every_ring_with_the_closing_position_deduplicated() {
        let hole = vec![
            Position::from([0.2, 0.2]),
            Position::from([0.4, 0.2]),
            Position::from([0.4, 0.4]),
            Position::from([0.2, 0.2]),
        ];
        let polygon = NgsiLdGeometry::Polygon {
            coordinates: vec![ring(), hole],
        };

        assert_eq!(
            vertices(polygon),
            Ok(NgsiLdGeometry::MultiPoint {
                coordinates: vec![
                    Position::from([0.0, 0.0]),
                    Position::from([1.0, 0.0]),
                    Position::from([1.0, 1.0]),
                    Position::from([0.2, 0.2]),
                    Position::from([0.4, 0.2]),
                    Position::from([0.4, 0.4]),
                ],
            })
        );
    }

    #[test]
    fn an_altitude_survives_the_move_into_a_multipoint() {
        let line = NgsiLdGeometry::LineString {
            coordinates: vec![Position::from([1.0, 2.0, 300.0]), Position::from([3.0, 4.0])],
        };

        assert_eq!(
            vertices(line),
            Ok(NgsiLdGeometry::MultiPoint {
                coordinates: vec![Position::from([1.0, 2.0, 300.0]), Position::from([3.0, 4.0])],
            })
        );
    }

    #[test]
    fn a_geometry_listing_no_position_yields_nothing() {
        assert_eq!(
            vertices(NgsiLdGeometry::MultiPoint { coordinates: Vec::new() }),
            Err(GeometryError::EmptyGeometry)
        );
    }

    #[test]
    fn the_first_position_is_read_from_the_deepest_first_member() {
        let multi = NgsiLdGeometry::MultiPolygon {
            coordinates: vec![vec![ring()]],
        };

        assert_eq!(first_position(&multi), Some(&Position::from([0.0, 0.0])));
        assert_eq!(first_position(&NgsiLdGeometry::MultiPoint { coordinates: Vec::new() }), None);
    }
}

use crate::{error::GeometryError, geometry::NgsiLdGeometry};
use geo_types::{Coord, Geometry as PlanarGeometry, LineString as PlanarLineString, MultiLineString, MultiPoint, Point, Polygon, Rect};
use geojson::{GeometryValue, LineStringType, Position};

/// Reads a geometry into `geo`'s planar model, the model every derivation computes in.
///
/// [`Coord`] is strictly two-dimensional, so the optional third element of a position (RFC 7946
/// clause 3.1.1) does not survive the trip. That is precisely why the lattice treats every
/// derivation as dropping altitude while selection and reshape keep it verbatim.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when a position carries too few components to be read as
/// a planar coordinate.
pub fn to_planar(geometry: &NgsiLdGeometry) -> Result<PlanarGeometry<f64>, GeometryError> {
    PlanarGeometry::try_from(&GeometryValue::from(geometry)).map_err(|_error| GeometryError::Unbuildable { target: geometry.kind() })
}

/// Reads one position as a planar coordinate, dropping any altitude.
#[must_use]
pub fn to_coord(position: &Position) -> Option<Coord<f64>> {
    match position.as_slice() {
        [x, y, ..] => Some(Coord { x: *x, y: *y }),
        [] | [_] => None,
    }
}

/// Reads a list of positions as a planar line string, refusing a position that is too short.
#[must_use]
pub fn to_planar_line(positions: &[Position]) -> Option<PlanarLineString<f64>> {
    positions.iter().map(to_coord).collect::<Option<Vec<Coord<f64>>>>().map(PlanarLineString::new)
}

/// Reads a polygon's rings as a planar polygon, refusing a position that is too short.
#[must_use]
pub fn to_planar_polygon(rings: &[LineStringType]) -> Option<Polygon<f64>> {
    let (exterior, interiors) = rings.split_first()?;
    let exterior = to_planar_line(exterior)?;
    let interiors = interiors
        .iter()
        .map(|ring| to_planar_line(ring))
        .collect::<Option<Vec<PlanarLineString<f64>>>>()?;

    Some(Polygon::new(exterior, interiors))
}

/// Writes a planar point back as a `Point` geometry.
#[must_use]
pub fn from_point(point: &Point<f64>) -> NgsiLdGeometry {
    NgsiLdGeometry::Point {
        coordinates: [point.x(), point.y()].into(),
    }
}

/// Writes planar coordinates back as a `MultiPoint` geometry.
#[must_use]
pub fn from_multi_point(points: &MultiPoint<f64>) -> NgsiLdGeometry {
    NgsiLdGeometry::MultiPoint {
        coordinates: points.iter().map(|point| [point.x(), point.y()].into()).collect(),
    }
}

/// Writes a planar line string back as a `LineString` geometry.
#[must_use]
pub fn from_line(line: &PlanarLineString<f64>) -> NgsiLdGeometry {
    NgsiLdGeometry::LineString {
        coordinates: from_line_coordinates(line),
    }
}

/// Writes a planar multi line string back as a `MultiLineString` geometry.
#[must_use]
pub fn from_multi_line(lines: &MultiLineString<f64>) -> NgsiLdGeometry {
    NgsiLdGeometry::MultiLineString {
        coordinates: lines.iter().map(from_line_coordinates).collect(),
    }
}

/// Writes a planar polygon back as a `Polygon` geometry, exterior ring first.
#[must_use]
pub fn from_polygon(polygon: &Polygon<f64>) -> NgsiLdGeometry {
    let mut coordinates = Vec::with_capacity(1 + polygon.interiors().len());
    coordinates.push(from_line_coordinates(polygon.exterior()));
    coordinates.extend(polygon.interiors().iter().map(from_line_coordinates));

    NgsiLdGeometry::Polygon { coordinates }
}

/// Writes a planar bounding rectangle back as a closed, counterclockwise `Polygon` geometry.
#[must_use]
pub fn from_rect(rect: &Rect<f64>) -> NgsiLdGeometry {
    from_polygon(&rect.to_polygon())
}

/// Copies a planar line string's coordinates into RFC 7946 positions.
fn from_line_coordinates(line: &PlanarLineString<f64>) -> LineStringType {
    line.coords().map(|coord| [coord.x, coord.y].into()).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::NgsiLdGeometry,
        planar::{from_polygon, to_coord, to_planar, to_planar_polygon},
    };
    use geo_types::Geometry as PlanarGeometry;
    use geojson::Position;

    #[test]
    fn a_three_component_position_reads_as_a_planar_coordinate_without_its_altitude() {
        let coord = to_coord(&Position::from([1.0, 2.0, 300.0])).expect("readable position");

        assert!((coord.x - 1.0).abs() < f64::EPSILON);
        assert!((coord.y - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_one_component_position_cannot_be_read() {
        assert!(to_coord(&Position::from(vec![1.0])).is_none());
    }

    #[test]
    fn a_geometry_reads_into_the_planar_model_and_back() {
        let rings = vec![vec![
            Position::from([0.0, 0.0]),
            Position::from([1.0, 0.0]),
            Position::from([1.0, 1.0]),
            Position::from([0.0, 0.0]),
        ]];
        let planar = to_planar_polygon(&rings).expect("readable rings");

        assert_eq!(from_polygon(&planar), NgsiLdGeometry::Polygon { coordinates: rings });
    }

    #[test]
    fn the_whole_geometry_reads_into_the_planar_model() {
        let geometry = NgsiLdGeometry::Point {
            coordinates: [1.0, 2.0].into(),
        };

        assert!(matches!(to_planar(&geometry), Ok(PlanarGeometry::Point(_))));
    }
}

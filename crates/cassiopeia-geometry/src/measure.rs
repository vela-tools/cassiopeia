use crate::{
    geometry::NgsiLdGeometry,
    planar::{to_planar_line, to_planar_polygon},
};
use geo::{Geodesic, GeodesicArea, Length};
use geo_types::Polygon;
use geojson::{LineStringType, Position};

/// The signed area a linear ring encloses, in square metres.
///
/// RFC 7946 clause 4 fixes the coordinate reference system to WGS84 longitude/latitude, so a planar
/// shoelace area would be measured in square degrees and would be meaningless. The measurement is
/// therefore geodesic, on the ellipsoid, by the method of Karney (2013); its sign follows the
/// winding, positive for a counterclockwise ring, which is what clause 3.1.6's right-hand rule asks
/// of an exterior ring.
///
/// A ring carrying a position too short to read measures zero: an unreadable ring has no winding to
/// preserve, and [`validate_structure`](crate::geometry::NgsiLdGeometry::validate_structure) rejects
/// it separately.
#[must_use]
pub fn signed_ring_area(ring: &[Position]) -> f64 {
    match to_planar_line(ring) {
        Some(line) => Polygon::new(line, Vec::new()).geodesic_area_signed(),
        None => 0.0,
    }
}

/// The area a geometry encloses, in square metres, measured geodesically.
///
/// A geometry of dimension below two encloses no area and measures zero.
#[must_use]
pub fn area(geometry: &NgsiLdGeometry) -> f64 {
    match geometry {
        NgsiLdGeometry::Point { .. } | NgsiLdGeometry::MultiPoint { .. } | NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::MultiLineString { .. } => 0.0,
        NgsiLdGeometry::Polygon { coordinates } => polygon_area(coordinates),
        NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.iter().map(|rings| polygon_area(rings)).sum(),
    }
}

/// The length of a geometry, in metres, measured geodesically.
///
/// A curve measures its own length and a surface measures its perimeter, holes included; a geometry
/// of dimension zero measures nothing.
#[must_use]
pub fn length(geometry: &NgsiLdGeometry) -> f64 {
    match geometry {
        NgsiLdGeometry::Point { .. } | NgsiLdGeometry::MultiPoint { .. } => 0.0,
        NgsiLdGeometry::LineString { coordinates } => line_length(coordinates),
        NgsiLdGeometry::MultiLineString { coordinates } => coordinates.iter().map(|line| line_length(line)).sum(),
        NgsiLdGeometry::Polygon { coordinates } => polygon_perimeter(coordinates),
        NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.iter().map(|rings| polygon_perimeter(rings)).sum(),
    }
}

/// The geodesic length of one curve, in metres, zero when a position cannot be read.
///
/// This is the metric `largest` orders a `MultiLineString`'s members by.
#[must_use]
pub fn line_length(positions: &[Position]) -> f64 {
    to_planar_line(positions).map_or(0.0, |line| Geodesic.length(&line))
}

/// The geodesic area one polygon encloses, in square metres, holes subtracted, zero when a position
/// cannot be read.
///
/// This is the metric `largest` orders a `MultiPolygon`'s members by.
///
/// The magnitude comes from the *signed* area rather than `geo`'s unsigned one, which assumes a
/// counterclockwise exterior ring and reports the area of the polygon's complement (very nearly the
/// whole Earth) for a clockwise one. A source is under no obligation to have wound its rings the
/// right way, and this runs on the geometry as the source wrote it, before winding is normalised.
#[must_use]
pub fn polygon_area(rings: &[LineStringType]) -> f64 {
    to_planar_polygon(rings).map_or(0.0, |polygon| polygon.geodesic_area_signed().abs())
}

/// The geodesic perimeter of one polygon, holes included, zero when a position cannot be read.
fn polygon_perimeter(rings: &[LineStringType]) -> f64 {
    to_planar_polygon(rings).map_or(0.0, |polygon| polygon.geodesic_perimeter())
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::NgsiLdGeometry,
        measure::{area, length, signed_ring_area},
    };
    use geojson::Position;

    /// A one-degree square with its exterior ring wound counterclockwise, near the equator.
    fn counterclockwise_ring() -> Vec<Position> {
        vec![
            Position::from([0.0, 0.0]),
            Position::from([1.0, 0.0]),
            Position::from([1.0, 1.0]),
            Position::from([0.0, 1.0]),
            Position::from([0.0, 0.0]),
        ]
    }

    #[test]
    fn a_counterclockwise_ring_measures_a_positive_area_and_its_reverse_a_negative_one() {
        let ring = counterclockwise_ring();
        let mut reversed = ring.clone();
        reversed.reverse();

        assert!(signed_ring_area(&ring) > 0.0);
        assert!(signed_ring_area(&reversed) < 0.0);
    }

    #[test]
    fn a_one_degree_square_at_the_equator_measures_roughly_twelve_thousand_square_kilometres() {
        let polygon = NgsiLdGeometry::Polygon {
            coordinates: vec![counterclockwise_ring()],
        };

        // A degree of longitude at the equator is about 111 km, so the square is about 12,300 km².
        let square_kilometres = area(&polygon) / 1_000_000.0;
        assert!((12_000.0..13_000.0).contains(&square_kilometres), "measured {square_kilometres} km²");
    }

    #[test]
    fn a_clockwise_ring_measures_its_own_area_and_not_the_earth_minus_it() {
        let mut clockwise = counterclockwise_ring();
        clockwise.reverse();
        let wound_either_way = [
            NgsiLdGeometry::Polygon {
                coordinates: vec![counterclockwise_ring()],
            },
            NgsiLdGeometry::Polygon { coordinates: vec![clockwise] },
        ];

        // `geo`'s unsigned geodesic area assumes a counterclockwise exterior ring and reports the
        // complement for a clockwise one, which would rank a tiny clockwise island above a continent.
        for polygon in wound_either_way {
            let square_kilometres = area(&polygon) / 1_000_000.0;
            assert!((12_000.0..13_000.0).contains(&square_kilometres), "measured {square_kilometres} km²");
        }
    }

    #[test]
    fn a_geometry_below_dimension_two_encloses_no_area() {
        let line = NgsiLdGeometry::LineString {
            coordinates: vec![Position::from([0.0, 0.0]), Position::from([1.0, 0.0])],
        };

        assert!(area(&line).abs() < f64::EPSILON);
    }

    #[test]
    fn a_degree_of_longitude_at_the_equator_measures_roughly_one_hundred_and_eleven_kilometres() {
        let line = NgsiLdGeometry::LineString {
            coordinates: vec![Position::from([0.0, 0.0]), Position::from([1.0, 0.0])],
        };

        let kilometres = length(&line) / 1000.0;
        assert!((111.0..112.0).contains(&kilometres), "measured {kilometres} km");
    }

    #[test]
    fn a_point_has_no_length() {
        let point = NgsiLdGeometry::Point {
            coordinates: [1.0, 2.0].into(),
        };

        assert!(length(&point).abs() < f64::EPSILON);
    }

    #[test]
    fn a_polygon_measures_its_perimeter_as_its_length() {
        let polygon = NgsiLdGeometry::Polygon {
            coordinates: vec![counterclockwise_ring()],
        };

        let kilometres = length(&polygon) / 1000.0;
        assert!((440.0..450.0).contains(&kilometres), "measured {kilometres} km");
    }
}

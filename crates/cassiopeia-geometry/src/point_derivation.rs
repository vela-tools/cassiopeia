use crate::{
    error::GeometryError,
    geometry::NgsiLdGeometry,
    planar::{from_point, to_planar},
    vertex_derivation::first_position,
};
use geo::{BoundingRect, Centroid, InteriorPoint};
use geo_types::Point;

/// Takes the geometry's centroid.
///
/// The computation is planar, on longitude and latitude read as plane coordinates, so it drifts at
/// high latitude and gives a meaningless result for a geometry spanning the antimeridian. It may
/// also fall outside a concave surface; `point-on-surface` is the conversion that cannot.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when a position cannot be read, and
/// [`GeometryError::EmptyGeometry`] when the geometry carries no coordinate to average.
pub fn centroid(geometry: &NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    to_planar(geometry)?
        .centroid()
        .map(|point| from_point(&point))
        .ok_or(GeometryError::EmptyGeometry)
}

/// Takes a position guaranteed to lie on or inside the geometry.
///
/// Planar, with the same caveats as [`centroid`], but unlike a centroid it is always on the
/// geometry, which is what a map pin for a concave administrative area needs.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when a position cannot be read, and
/// [`GeometryError::EmptyGeometry`] when the geometry carries no coordinate to place a point on.
pub fn point_on_surface(geometry: &NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    to_planar(geometry)?
        .interior_point()
        .map(|point| from_point(&point))
        .ok_or(GeometryError::EmptyGeometry)
}

/// Takes the first position the geometry lists, verbatim.
///
/// The position is copied rather than computed, so its altitude (RFC 7946 clause 3.1.1) survives.
///
/// # Errors
/// Returns [`GeometryError::EmptyGeometry`] when the geometry lists no position.
pub fn first_vertex(geometry: &NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    first_position(geometry)
        .map(|position| NgsiLdGeometry::Point { coordinates: position.clone() })
        .ok_or(GeometryError::EmptyGeometry)
}

/// Takes the centre of the geometry's bounding box.
///
/// # Errors
/// Returns [`GeometryError::Unbuildable`] when a position cannot be read, and
/// [`GeometryError::EmptyGeometry`] when the geometry has no extent to bound.
pub fn bbox_center(geometry: &NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    let rect = to_planar(geometry)?.bounding_rect().ok_or(GeometryError::EmptyGeometry)?;

    Ok(from_point(&Point::from(rect.center())))
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::NgsiLdGeometry,
        point_derivation::{bbox_center, centroid, first_vertex, point_on_surface},
    };
    use geojson::Position;

    /// An L-shaped surface whose centroid falls outside it.
    fn concave() -> NgsiLdGeometry {
        NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                Position::from([0.0, 0.0]),
                Position::from([3.0, 0.0]),
                Position::from([3.0, 1.0]),
                Position::from([1.0, 1.0]),
                Position::from([1.0, 3.0]),
                Position::from([0.0, 3.0]),
                Position::from([0.0, 0.0]),
            ]],
        }
    }

    fn coordinates(geometry: &NgsiLdGeometry) -> Vec<f64> {
        match geometry {
            NgsiLdGeometry::Point { coordinates } => coordinates.as_slice().to_vec(),
            NgsiLdGeometry::MultiPoint { .. }
            | NgsiLdGeometry::LineString { .. }
            | NgsiLdGeometry::MultiLineString { .. }
            | NgsiLdGeometry::Polygon { .. }
            | NgsiLdGeometry::MultiPolygon { .. } => Vec::new(),
        }
    }

    #[test]
    fn the_centroid_of_a_square_is_its_middle() {
        let square = NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                Position::from([0.0, 0.0]),
                Position::from([2.0, 0.0]),
                Position::from([2.0, 2.0]),
                Position::from([0.0, 2.0]),
                Position::from([0.0, 0.0]),
            ]],
        };
        let point = centroid(&square).expect("a square has a centroid");

        assert_eq!(coordinates(&point), vec![1.0, 1.0]);
    }

    #[test]
    fn a_point_on_surface_lands_inside_a_concave_surface() {
        let point = point_on_surface(&concave()).expect("a surface has an interior point");
        let coordinates = coordinates(&point);

        // The L's notch spans x > 1 and y > 1; an interior point must not land in it.
        assert!(coordinates[0] <= 1.0 || coordinates[1] <= 1.0, "landed in the notch at {coordinates:?}");
    }

    #[test]
    fn the_first_vertex_is_copied_with_its_altitude() {
        let line = NgsiLdGeometry::LineString {
            coordinates: vec![Position::from([1.0, 2.0, 300.0]), Position::from([3.0, 4.0])],
        };

        assert_eq!(
            first_vertex(&line),
            Ok(NgsiLdGeometry::Point {
                coordinates: Position::from([1.0, 2.0, 300.0]),
            })
        );
    }

    #[test]
    fn the_bounding_box_centre_of_an_l_shape_is_the_middle_of_its_extent() {
        let point = bbox_center(&concave()).expect("a surface has a bounding box");

        assert_eq!(coordinates(&point), vec![1.5, 1.5]);
    }
}

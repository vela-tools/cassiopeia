use crate::{
    error::GeometryError,
    geometry::{Dimension, NgsiLdGeometry},
};
use geojson::{Geometry, GeometryValue, LineStringType, PointType, PolygonType};

/// The members gathered while folding a `GeometryCollection`, one bucket per dimension.
///
/// A collection is folded into a single geometry only when every member spans the same dimension,
/// so the bucket that was opened by the first member also decides which later members are
/// admissible: anything landing in another bucket makes the collection mixed.
enum Fold {
    /// Positions gathered from `Point` and `MultiPoint` members.
    Points(Vec<PointType>),
    /// Curves gathered from `LineString` and `MultiLineString` members.
    Lines(Vec<LineStringType>),
    /// Surfaces gathered from `Polygon` and `MultiPolygon` members.
    Areas(Vec<PolygonType>),
}

impl Fold {
    /// Opens the bucket for a dimension.
    const fn for_dimension(dimension: Dimension) -> Fold {
        match dimension {
            Dimension::Point => Fold::Points(Vec::new()),
            Dimension::Line => Fold::Lines(Vec::new()),
            Dimension::Area => Fold::Areas(Vec::new()),
        }
    }

    /// Adds one member's coordinates, refusing a member of another dimension.
    fn accept(&mut self, member: NgsiLdGeometry) -> Result<(), GeometryError> {
        match (self, member) {
            (Fold::Points(points), NgsiLdGeometry::Point { coordinates }) => points.push(coordinates),
            (Fold::Points(points), NgsiLdGeometry::MultiPoint { coordinates }) => points.extend(coordinates),
            (Fold::Lines(lines), NgsiLdGeometry::LineString { coordinates }) => lines.push(coordinates),
            (Fold::Lines(lines), NgsiLdGeometry::MultiLineString { coordinates }) => lines.extend(coordinates),
            (Fold::Areas(areas), NgsiLdGeometry::Polygon { coordinates }) => areas.push(coordinates),
            (Fold::Areas(areas), NgsiLdGeometry::MultiPolygon { coordinates }) => areas.extend(coordinates),
            (
                Fold::Points(_) | Fold::Lines(_) | Fold::Areas(_),
                NgsiLdGeometry::Point { .. }
                | NgsiLdGeometry::MultiPoint { .. }
                | NgsiLdGeometry::LineString { .. }
                | NgsiLdGeometry::MultiLineString { .. }
                | NgsiLdGeometry::Polygon { .. }
                | NgsiLdGeometry::MultiPolygon { .. },
            ) => return Err(GeometryError::MixedCollection),
        }

        Ok(())
    }

    /// Closes the bucket into the multi-geometry of its dimension.
    fn into_geometry(self) -> NgsiLdGeometry {
        match self {
            Fold::Points(coordinates) => NgsiLdGeometry::MultiPoint { coordinates },
            Fold::Lines(coordinates) => NgsiLdGeometry::MultiLineString { coordinates },
            Fold::Areas(coordinates) => NgsiLdGeometry::MultiPolygon { coordinates },
        }
    }
}

impl TryFrom<GeometryValue> for NgsiLdGeometry {
    type Error = GeometryError;

    /// Admits a `GeoJSON` geometry, refusing the one type a `GeoProperty` cannot hold.
    ///
    /// ETSI GS CIM 009 v1.9.1 clause 4.7 admits six of RFC 7946's seven geometry types; a
    /// `GeometryCollection` (RFC 7946 clause 3.1.8) is refused here rather than being carried and
    /// rejected downstream.
    fn try_from(value: GeometryValue) -> Result<NgsiLdGeometry, GeometryError> {
        match value {
            GeometryValue::Point { coordinates } => Ok(NgsiLdGeometry::Point { coordinates }),
            GeometryValue::MultiPoint { coordinates } => Ok(NgsiLdGeometry::MultiPoint { coordinates }),
            GeometryValue::LineString { coordinates } => Ok(NgsiLdGeometry::LineString { coordinates }),
            GeometryValue::MultiLineString { coordinates } => Ok(NgsiLdGeometry::MultiLineString { coordinates }),
            GeometryValue::Polygon { coordinates } => Ok(NgsiLdGeometry::Polygon { coordinates }),
            GeometryValue::MultiPolygon { coordinates } => Ok(NgsiLdGeometry::MultiPolygon { coordinates }),
            GeometryValue::GeometryCollection { .. } => Err(GeometryError::GeometryCollection),
        }
    }
}

impl TryFrom<Geometry> for NgsiLdGeometry {
    type Error = GeometryError;

    /// Admits a `GeoJSON` geometry object, dropping its `bbox` (RFC 7946 clause 5) and foreign
    /// members (clause 6.1), which carry no coordinates a `GeoProperty` value is defined over.
    fn try_from(geometry: Geometry) -> Result<NgsiLdGeometry, GeometryError> {
        NgsiLdGeometry::try_from(geometry.value)
    }
}

impl From<NgsiLdGeometry> for GeometryValue {
    fn from(geometry: NgsiLdGeometry) -> GeometryValue {
        match geometry {
            NgsiLdGeometry::Point { coordinates } => GeometryValue::Point { coordinates },
            NgsiLdGeometry::MultiPoint { coordinates } => GeometryValue::MultiPoint { coordinates },
            NgsiLdGeometry::LineString { coordinates } => GeometryValue::LineString { coordinates },
            NgsiLdGeometry::MultiLineString { coordinates } => GeometryValue::MultiLineString { coordinates },
            NgsiLdGeometry::Polygon { coordinates } => GeometryValue::Polygon { coordinates },
            NgsiLdGeometry::MultiPolygon { coordinates } => GeometryValue::MultiPolygon { coordinates },
        }
    }
}

impl From<&NgsiLdGeometry> for GeometryValue {
    /// Copies the geometry into `geojson`'s own value type.
    ///
    /// The coordinates are cloned because `GeometryValue` owns them and the source is borrowed; this
    /// backs `Display` and the planar conversions, neither of which can consume the geometry they
    /// render or measure.
    fn from(geometry: &NgsiLdGeometry) -> GeometryValue {
        match geometry {
            NgsiLdGeometry::Point { coordinates } => GeometryValue::Point {
                coordinates: coordinates.clone(),
            },
            NgsiLdGeometry::MultiPoint { coordinates } => GeometryValue::MultiPoint {
                coordinates: coordinates.clone(),
            },
            NgsiLdGeometry::LineString { coordinates } => GeometryValue::LineString {
                coordinates: coordinates.clone(),
            },
            NgsiLdGeometry::MultiLineString { coordinates } => GeometryValue::MultiLineString {
                coordinates: coordinates.clone(),
            },
            NgsiLdGeometry::Polygon { coordinates } => GeometryValue::Polygon {
                coordinates: coordinates.clone(),
            },
            NgsiLdGeometry::MultiPolygon { coordinates } => GeometryValue::MultiPolygon {
                coordinates: coordinates.clone(),
            },
        }
    }
}

/// Folds a `GeometryCollection`'s members into one admissible geometry.
///
/// This is what the opt-in `flatten` conversion performs on a source RFC 7946 clause 3.1.8 permits
/// but clause 4.7 of ETSI GS CIM 009 v1.9.1 does not: a lone member becomes that geometry, members
/// that all span the same dimension become the multi-geometry of that dimension, and a nested
/// collection is folded first and contributes its own members.
///
/// # Errors
/// Returns [`GeometryError::EmptyGeometry`] for a collection with no members, and
/// [`GeometryError::MixedCollection`] when its members span more than one dimension.
pub fn fold_collection(geometries: Vec<Geometry>) -> Result<NgsiLdGeometry, GeometryError> {
    let mut members = Vec::with_capacity(geometries.len());
    for geometry in geometries {
        members.push(admit(geometry)?);
    }

    fold(members)
}

/// Admits one collection member, folding it first when it is itself a collection.
fn admit(geometry: Geometry) -> Result<NgsiLdGeometry, GeometryError> {
    match geometry.value {
        GeometryValue::GeometryCollection { geometries } => fold_collection(geometries),
        value @ (GeometryValue::Point { .. }
        | GeometryValue::MultiPoint { .. }
        | GeometryValue::LineString { .. }
        | GeometryValue::MultiLineString { .. }
        | GeometryValue::Polygon { .. }
        | GeometryValue::MultiPolygon { .. }) => NgsiLdGeometry::try_from(value),
    }
}

/// Merges already-admitted members into one geometry, keeping a lone member's own type.
fn fold(members: Vec<NgsiLdGeometry>) -> Result<NgsiLdGeometry, GeometryError> {
    let mut members = members.into_iter();
    let Some(first) = members.next() else {
        return Err(GeometryError::EmptyGeometry);
    };

    let mut fold = Fold::for_dimension(first.kind().dimension());
    let mut only = Some(first);
    for member in members {
        if let Some(first) = only.take() {
            fold.accept(first)?;
        }
        fold.accept(member)?;
    }

    match only {
        Some(lone) => Ok(lone),
        None => Ok(fold.into_geometry()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::GeometryError, geometry::NgsiLdGeometry, rfc7946::fold_collection};
    use geojson::{Geometry, GeometryValue};

    fn point(x: f64, y: f64) -> Geometry {
        Geometry::new(GeometryValue::Point { coordinates: [x, y].into() })
    }

    fn line() -> Geometry {
        Geometry::new(GeometryValue::LineString {
            coordinates: vec![[0.0, 0.0].into(), [1.0, 1.0].into()],
        })
    }

    #[test]
    fn a_geometry_collection_is_refused_at_the_boundary() {
        let collection = GeometryValue::GeometryCollection {
            geometries: vec![point(1.0, 2.0)],
        };

        assert_eq!(NgsiLdGeometry::try_from(collection), Err(GeometryError::GeometryCollection));
    }

    #[test]
    fn a_geometry_object_loses_its_bbox_and_foreign_members() {
        let mut geometry = point(1.0, 2.0);
        geometry.bbox = Some(vec![1.0, 2.0, 1.0, 2.0]);

        assert_eq!(
            NgsiLdGeometry::try_from(geometry),
            Ok(NgsiLdGeometry::Point {
                coordinates: [1.0, 2.0].into()
            })
        );
    }

    #[test]
    fn folding_a_lone_member_yields_that_member_unchanged() {
        assert_eq!(
            fold_collection(vec![line()]),
            Ok(NgsiLdGeometry::LineString {
                coordinates: vec![[0.0, 0.0].into(), [1.0, 1.0].into()],
            })
        );
    }

    #[test]
    fn folding_homogeneous_members_yields_the_multi_geometry_of_their_dimension() {
        assert_eq!(
            fold_collection(vec![point(1.0, 2.0), point(3.0, 4.0)]),
            Ok(NgsiLdGeometry::MultiPoint {
                coordinates: vec![[1.0, 2.0].into(), [3.0, 4.0].into()],
            })
        );
    }

    #[test]
    fn folding_a_nested_collection_contributes_its_own_members() {
        let nested = Geometry::new(GeometryValue::GeometryCollection {
            geometries: vec![point(3.0, 4.0), point(5.0, 6.0)],
        });

        assert_eq!(
            fold_collection(vec![point(1.0, 2.0), nested]),
            Ok(NgsiLdGeometry::MultiPoint {
                coordinates: vec![[1.0, 2.0].into(), [3.0, 4.0].into(), [5.0, 6.0].into()],
            })
        );
    }

    #[test]
    fn folding_members_of_different_dimensions_is_refused() {
        assert_eq!(fold_collection(vec![point(1.0, 2.0), line()]), Err(GeometryError::MixedCollection));
    }

    #[test]
    fn folding_an_empty_collection_fabricates_nothing() {
        assert_eq!(fold_collection(Vec::new()), Err(GeometryError::EmptyGeometry));
    }
}

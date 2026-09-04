use ::geojson::{Geometry as GeoJsonGeometry, GeometryValue, LineStringType, PointType, PolygonType};
use ::kml::types::Geometry as KmlGeometry;
use serde_json::Value;

/// The members gathered while folding a KML `MultiGeometry`, one bucket per geometry family.
///
/// KML's `MultiGeometry` (OGC 07-147r2 clause 10.1) may hold any mix of geometries, while `GeoJSON`'s
/// multi-geometries each hold one family. A `MultiGeometry` whose members all belong to one family
/// therefore has an exact `GeoJSON` counterpart, and one that mixes families has none.
enum MultiGeometryFold {
    /// Positions gathered from `Point` members.
    Points(Vec<PointType>),
    /// Curves gathered from `LineString` and `LinearRing` members.
    Lines(Vec<LineStringType>),
    /// Surfaces gathered from `Polygon` members.
    Polygons(Vec<PolygonType>),
}

impl MultiGeometryFold {
    /// Opens the bucket the first member belongs to, or `None` for a member with no family.
    fn open(value: GeometryValue) -> Option<MultiGeometryFold> {
        let mut fold = match &value {
            GeometryValue::Point { .. } | GeometryValue::MultiPoint { .. } => MultiGeometryFold::Points(Vec::new()),
            GeometryValue::LineString { .. } | GeometryValue::MultiLineString { .. } => MultiGeometryFold::Lines(Vec::new()),
            GeometryValue::Polygon { .. } | GeometryValue::MultiPolygon { .. } => MultiGeometryFold::Polygons(Vec::new()),
            GeometryValue::GeometryCollection { .. } => return None,
        };
        fold.accept(value)?;

        Some(fold)
    }

    /// Adds one member's coordinates, refusing a member of another family.
    ///
    /// A nested `MultiGeometry` has already been folded into a multi-geometry of its own by the time
    /// it arrives here, so its members are spliced into this fold rather than nested inside it.
    fn accept(&mut self, value: GeometryValue) -> Option<()> {
        match (self, value) {
            (MultiGeometryFold::Points(points), GeometryValue::Point { coordinates }) => points.push(coordinates),
            (MultiGeometryFold::Points(points), GeometryValue::MultiPoint { coordinates }) => points.extend(coordinates),
            (MultiGeometryFold::Lines(lines), GeometryValue::LineString { coordinates }) => lines.push(coordinates),
            (MultiGeometryFold::Lines(lines), GeometryValue::MultiLineString { coordinates }) => lines.extend(coordinates),
            (MultiGeometryFold::Polygons(polygons), GeometryValue::Polygon { coordinates }) => polygons.push(coordinates),
            (MultiGeometryFold::Polygons(polygons), GeometryValue::MultiPolygon { coordinates }) => polygons.extend(coordinates),
            (
                MultiGeometryFold::Points(_) | MultiGeometryFold::Lines(_) | MultiGeometryFold::Polygons(_),
                GeometryValue::Point { .. }
                | GeometryValue::MultiPoint { .. }
                | GeometryValue::LineString { .. }
                | GeometryValue::MultiLineString { .. }
                | GeometryValue::Polygon { .. }
                | GeometryValue::MultiPolygon { .. }
                | GeometryValue::GeometryCollection { .. },
            ) => return None,
        }

        Some(())
    }

    /// Closes the bucket into the `GeoJSON` multi-geometry of its family.
    fn into_value(self) -> GeometryValue {
        match self {
            MultiGeometryFold::Points(coordinates) => GeometryValue::MultiPoint { coordinates },
            MultiGeometryFold::Lines(coordinates) => GeometryValue::MultiLineString { coordinates },
            MultiGeometryFold::Polygons(coordinates) => GeometryValue::MultiPolygon { coordinates },
        }
    }
}

/// Converts a KML geometry into a `GeoJSON`-compatible JSON value.
///
/// Returns `None` for geometry kinds that have no `GeoJSON` equivalent. KML's
/// `LinearRing` maps to a `GeoJSON` `LineString`, matching the source semantics of
/// a standalone ring.
///
/// A `MultiGeometry` is folded into the `GeoJSON` multi-geometry of its members' family rather than
/// into a `GeometryCollection`, because a collection is not one of the geometry types an NGSI-LD
/// `GeoProperty` admits (ETSI GS CIM 009 v1.9.1 clause 4.7). A `MultiGeometry` of one member becomes
/// that member, and one mixing families has no equivalent and yields `None`, exactly as the kinds
/// with no `GeoJSON` counterpart do.
#[must_use]
pub fn kml_geometry_to_geojson(geometry: &KmlGeometry) -> Option<Value> {
    let geojson_geom = GeoJsonGeometry::new(to_geometry_value(geometry)?);

    serde_json::to_value(&geojson_geom).ok()
}

/// Converts a KML geometry into a `GeoJSON` geometry value, folding a `MultiGeometry` as it goes.
fn to_geometry_value(geometry: &KmlGeometry) -> Option<GeometryValue> {
    match geometry {
        KmlGeometry::Point(p) => Some(GeometryValue::Point {
            coordinates: vec![p.coord.x, p.coord.y].into(),
        }),
        KmlGeometry::LineString(ls) => Some(GeometryValue::LineString {
            coordinates: ls.coords.iter().map(|c| vec![c.x, c.y].into()).collect(),
        }),
        KmlGeometry::LinearRing(lr) => Some(GeometryValue::LineString {
            coordinates: lr.coords.iter().map(|c| vec![c.x, c.y].into()).collect(),
        }),
        KmlGeometry::Polygon(poly) => {
            let mut rings = vec![poly.outer.coords.iter().map(|c| vec![c.x, c.y].into()).collect()];
            for inner in &poly.inner {
                rings.push(inner.coords.iter().map(|c| vec![c.x, c.y].into()).collect());
            }
            Some(GeometryValue::Polygon { coordinates: rings })
        }
        KmlGeometry::MultiGeometry(mg) => fold_multi_geometry(mg.geometries.iter().filter_map(to_geometry_value)),
        // `kml::types::Geometry` is `#[non_exhaustive]`, so the trailing wildcard is
        // required to cover hidden future kinds; the remaining known kinds have no
        // GeoJSON geometry equivalent.
        KmlGeometry::Element(_) | _ => None,
    }
}

/// Folds a `MultiGeometry`'s already-converted members into one `GeoJSON` geometry.
///
/// A lone member keeps its own geometry type: wrapping it in a multi-geometry would say something
/// the source did not.
fn fold_multi_geometry(members: impl Iterator<Item = GeometryValue>) -> Option<GeometryValue> {
    let mut members = members.peekable();
    let first = members.next()?;
    if members.peek().is_none() {
        return Some(first);
    }

    let mut fold = MultiGeometryFold::open(first)?;
    for member in members {
        fold.accept(member)?;
    }

    Some(fold.into_value())
}

#[cfg(test)]
mod tests {
    use crate::kml::geometry::kml_geometry_to_geojson;
    use ::kml::types::{Coord, Geometry as KmlGeometry, LineString, LinearRing, MultiGeometry, Point, Polygon};

    fn point(x: f64, y: f64) -> KmlGeometry {
        KmlGeometry::Point(Point::new(x, y, None))
    }

    fn line() -> KmlGeometry {
        KmlGeometry::LineString(LineString::from(vec![Coord::new(0.0, 0.0, None), Coord::new(1.0, 1.0, None)]))
    }

    fn ring() -> LinearRing {
        LinearRing::from(vec![
            Coord::new(0.0, 0.0, None),
            Coord::new(1.0, 0.0, None),
            Coord::new(1.0, 1.0, None),
            Coord::new(0.0, 0.0, None),
        ])
    }

    #[test]
    fn a_multi_geometry_of_points_folds_into_a_multipoint() {
        let multi = KmlGeometry::MultiGeometry(MultiGeometry::new(vec![point(1.0, 2.0), point(3.0, 4.0)]));
        let json = kml_geometry_to_geojson(&multi).expect("a GeoJSON geometry");

        assert_eq!(json["type"], "MultiPoint");
        assert_eq!(json["coordinates"].as_array().expect("coordinates").len(), 2);
    }

    #[test]
    fn a_multi_geometry_of_polygons_folds_into_a_multipolygon() {
        let polygon = KmlGeometry::Polygon(Polygon::new(ring(), Vec::new()));
        let multi = KmlGeometry::MultiGeometry(MultiGeometry::new(vec![polygon]));
        let json = kml_geometry_to_geojson(&multi).expect("a GeoJSON geometry");

        // A lone member keeps its own type rather than being wrapped.
        assert_eq!(json["type"], "Polygon");
    }

    #[test]
    fn a_nested_multi_geometry_contributes_to_the_same_fold() {
        let nested = KmlGeometry::MultiGeometry(MultiGeometry::new(vec![point(3.0, 4.0), point(5.0, 6.0)]));
        let multi = KmlGeometry::MultiGeometry(MultiGeometry::new(vec![point(1.0, 2.0), nested]));
        let json = kml_geometry_to_geojson(&multi).expect("a GeoJSON geometry");

        assert_eq!(json["type"], "MultiPoint");
        assert_eq!(json["coordinates"].as_array().expect("coordinates").len(), 3);
    }

    #[test]
    fn a_multi_geometry_mixing_families_has_no_geojson_equivalent() {
        let multi = KmlGeometry::MultiGeometry(MultiGeometry::new(vec![point(1.0, 2.0), line()]));

        assert!(kml_geometry_to_geojson(&multi).is_none());
    }

    #[test]
    fn no_multi_geometry_ever_produces_a_geometry_collection() {
        let multi = KmlGeometry::MultiGeometry(MultiGeometry::new(vec![line(), line()]));
        let json = kml_geometry_to_geojson(&multi).expect("a GeoJSON geometry");

        assert_eq!(json["type"], "MultiLineString");
    }

    #[test]
    fn a_linear_ring_maps_to_a_line_string() {
        let json = kml_geometry_to_geojson(&KmlGeometry::LinearRing(ring())).expect("a GeoJSON geometry");

        assert_eq!(json["type"], "LineString");
    }
}

use derive_more::Display;
use geojson::{GeometryValue, LineStringType, PointType, PolygonType};
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

/// How many dimensions a geometry's coordinates span.
///
/// The lattice is organised around this: a conversion that keeps the dimension only regroups
/// coordinates, while one that changes it either discards extent or derives it from vertices.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Dimension {
    /// A position, carrying no extent.
    Point,
    /// A curve, carrying length but no area.
    Line,
    /// A surface, carrying area.
    Area,
}

/// Whether a geometry type holds one member or an ordered list of them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Cardinality {
    /// One member: `Point`, `LineString`, `Polygon`.
    Single,
    /// Zero or more members: `MultiPoint`, `MultiLineString`, `MultiPolygon`.
    Multi,
}

/// A `GeoJSON` geometry restricted to the types an NGSI-LD `GeoProperty` admits.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.7 admits exactly six of RFC 7946's geometry types, so the
/// seventh, `GeometryCollection` (RFC 7946 clause 3.1.8), has no variant here and cannot reach a
/// `GeoProperty` however the source spelled it.
///
/// The coordinate aliases come from `geojson` rather than being restated, so a position keeps the
/// optional third element RFC 7946 clause 3.1.1 permits: an altitude survives every conversion that
/// does not explicitly discard it. The serde shape reproduces `GeometryValue`'s own, so the emitted
/// bytes are identical and RFC 7946's `bbox` (clause 5) and foreign members (clause 6.1) are read as
/// unknown fields and dropped.
#[derive(Clone, Debug, Deserialize, Display, EnumDiscriminants, PartialEq, Serialize)]
#[display("{}", GeometryValue::from(self))]
#[serde(tag = "type")]
#[strum_discriminants(name(GeometryKind), derive(Hash, strum::Display, strum::EnumIter))]
pub enum NgsiLdGeometry {
    /// One position (RFC 7946 clause 3.1.2).
    Point {
        /// The position.
        coordinates: PointType,
    },
    /// An array of positions (RFC 7946 clause 3.1.3).
    MultiPoint {
        /// The positions.
        coordinates: Vec<PointType>,
    },
    /// A curve through two or more positions (RFC 7946 clause 3.1.4).
    LineString {
        /// The positions, in order.
        coordinates: LineStringType,
    },
    /// An array of curves (RFC 7946 clause 3.1.5).
    MultiLineString {
        /// The curves, in order.
        coordinates: Vec<LineStringType>,
    },
    /// A surface bounded by an exterior linear ring and any number of interior ones (RFC 7946
    /// clause 3.1.6).
    Polygon {
        /// The rings: the exterior one first, then the holes.
        coordinates: PolygonType,
    },
    /// An array of surfaces (RFC 7946 clause 3.1.7).
    MultiPolygon {
        /// The surfaces, in order.
        coordinates: Vec<PolygonType>,
    },
}

impl NgsiLdGeometry {
    /// Which of the six geometry types this value is.
    #[must_use]
    pub fn kind(&self) -> GeometryKind {
        GeometryKind::from(self)
    }

    /// How many members a multi-geometry carries; a single geometry always carries exactly one.
    #[must_use]
    pub const fn members(&self) -> usize {
        match self {
            NgsiLdGeometry::Point { .. } | NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::Polygon { .. } => 1,
            NgsiLdGeometry::MultiPoint { coordinates } => coordinates.len(),
            NgsiLdGeometry::MultiLineString { coordinates } => coordinates.len(),
            NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.len(),
        }
    }
}

impl GeometryKind {
    /// How many dimensions this geometry type's coordinates span.
    #[must_use]
    pub const fn dimension(self) -> Dimension {
        match self {
            GeometryKind::Point | GeometryKind::MultiPoint => Dimension::Point,
            GeometryKind::LineString | GeometryKind::MultiLineString => Dimension::Line,
            GeometryKind::Polygon | GeometryKind::MultiPolygon => Dimension::Area,
        }
    }

    /// Whether this geometry type holds one member or a list of them.
    #[must_use]
    pub const fn cardinality(self) -> Cardinality {
        match self {
            GeometryKind::Point | GeometryKind::LineString | GeometryKind::Polygon => Cardinality::Single,
            GeometryKind::MultiPoint | GeometryKind::MultiLineString | GeometryKind::MultiPolygon => Cardinality::Multi,
        }
    }

    /// The single-member type of the same dimension.
    #[must_use]
    pub const fn singular(self) -> GeometryKind {
        match self {
            GeometryKind::Point | GeometryKind::MultiPoint => GeometryKind::Point,
            GeometryKind::LineString | GeometryKind::MultiLineString => GeometryKind::LineString,
            GeometryKind::Polygon | GeometryKind::MultiPolygon => GeometryKind::Polygon,
        }
    }

    /// The multi-member type of the same dimension.
    #[must_use]
    pub const fn plural(self) -> GeometryKind {
        match self {
            GeometryKind::Point | GeometryKind::MultiPoint => GeometryKind::MultiPoint,
            GeometryKind::LineString | GeometryKind::MultiLineString => GeometryKind::MultiLineString,
            GeometryKind::Polygon | GeometryKind::MultiPolygon => GeometryKind::MultiPolygon,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::{Cardinality, Dimension, GeometryKind, NgsiLdGeometry};
    use serde_json::json;
    use strum::IntoEnumIterator;

    #[test]
    fn the_six_admissible_types_round_trip_through_their_rfc_7946_shape() {
        for (kind, document) in [
            (GeometryKind::Point, json!({"type": "Point", "coordinates": [1.0, 2.0]})),
            (GeometryKind::MultiPoint, json!({"type": "MultiPoint", "coordinates": [[1.0, 2.0]]})),
            (GeometryKind::LineString, json!({"type": "LineString", "coordinates": [[1.0, 2.0], [3.0, 4.0]]})),
            (
                GeometryKind::MultiLineString,
                json!({"type": "MultiLineString", "coordinates": [[[1.0, 2.0], [3.0, 4.0]]]}),
            ),
            (
                GeometryKind::Polygon,
                json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]}),
            ),
            (
                GeometryKind::MultiPolygon,
                json!({"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]}),
            ),
        ] {
            let geometry: NgsiLdGeometry = serde_json::from_value(document.clone()).expect("admissible geometry");
            assert_eq!(geometry.kind(), kind);
            assert_eq!(serde_json::to_value(&geometry).expect("serialises"), document);
        }
    }

    #[test]
    fn a_geometry_collection_has_no_variant_to_deserialise_into() {
        // Clause 4.7 admits six geometry types and no collection, so the type makes the seventh
        // unrepresentable rather than validating it away later.
        let document = json!({"type": "GeometryCollection", "geometries": []});

        assert!(serde_json::from_value::<NgsiLdGeometry>(document).is_err());
    }

    #[test]
    fn a_bbox_and_a_foreign_member_are_dropped_rather_than_carried() {
        let document = json!({"type": "Point", "coordinates": [1.0, 2.0], "bbox": [1.0, 2.0, 1.0, 2.0], "title": "here"});
        let geometry: NgsiLdGeometry = serde_json::from_value(document).expect("admissible geometry");

        assert_eq!(
            serde_json::to_value(&geometry).expect("serialises"),
            json!({"type": "Point", "coordinates": [1.0, 2.0]})
        );
    }

    #[test]
    fn an_altitude_survives_deserialisation() {
        let document = json!({"type": "Point", "coordinates": [1.0, 2.0, 300.0]});
        let geometry: NgsiLdGeometry = serde_json::from_value(document.clone()).expect("admissible geometry");

        assert_eq!(serde_json::to_value(&geometry).expect("serialises"), document);
    }

    #[test]
    fn display_renders_the_geometry_as_its_geojson_document() {
        let geometry = NgsiLdGeometry::Point {
            coordinates: [1.5, 2.5].into(),
        };

        assert_eq!(geometry.to_string(), r#"{"type":"Point","coordinates":[1.5,2.5]}"#);
    }

    #[test]
    fn every_kind_names_its_rfc_7946_token() {
        assert_eq!(GeometryKind::MultiLineString.to_string(), "MultiLineString");
        assert_eq!(GeometryKind::iter().count(), 6);
    }

    #[test]
    fn singular_and_plural_pair_every_kind_with_its_counterpart_of_the_same_dimension() {
        for kind in GeometryKind::iter() {
            assert_eq!(kind.singular().dimension(), kind.dimension());
            assert_eq!(kind.plural().dimension(), kind.dimension());
            assert_eq!(kind.singular().cardinality(), Cardinality::Single);
            assert_eq!(kind.plural().cardinality(), Cardinality::Multi);
        }
    }

    #[test]
    fn dimension_groups_the_kinds_into_points_lines_and_areas() {
        assert_eq!(GeometryKind::Point.dimension(), Dimension::Point);
        assert_eq!(GeometryKind::MultiLineString.dimension(), Dimension::Line);
        assert_eq!(GeometryKind::Polygon.dimension(), Dimension::Area);
    }

    #[test]
    fn members_counts_a_multi_geometry_and_reads_one_for_a_single_one() {
        let single = NgsiLdGeometry::Point {
            coordinates: [1.0, 2.0].into(),
        };
        let multi = NgsiLdGeometry::MultiPoint {
            coordinates: vec![[1.0, 2.0].into(), [3.0, 4.0].into()],
        };

        assert_eq!(single.members(), 1);
        assert_eq!(multi.members(), 2);
    }
}

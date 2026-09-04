use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};
use serde::{Deserialize, Serialize};

/// The conversion applied to an extracted source value before it becomes an NGSI-LD value.
///
/// The geometry variants name `GeoJSON` geometry types (RFC 7946 clause 3.1) and produce values
/// consumed by the corresponding NGSI-LD `GeoProperty` constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transformation {
    /// Parse as a boolean.
    #[serde(alias = "bool")]
    Boolean,

    /// Parse as a 64-bit signed integer.
    #[serde(alias = "int")]
    Integer,

    /// Parse as a double-precision float.
    Float,

    /// Keep as text.
    String,

    /// Parse as a JSON array.
    Array,

    /// Parse as a JSON object.
    Object,

    /// Accept an already-formed `GeoJSON` geometry, inferring the type from the value.
    ///
    /// Unlike the type-specific geometry transformations below, which build a geometry of a fixed
    /// type from raw coordinates, this keeps the geometry the source already carries (a `GeoJSON`
    /// geometry object, RFC 7946 clause 3.1). NGSI-LD restrictions are enforced separately.
    Geometry,

    /// Build a `GeoJSON` `Point`.
    Point,

    /// Build a `GeoJSON` `MultiPoint`.
    MultiPoint,

    /// Build a `GeoJSON` `LineString`.
    LineString,

    /// Build a `GeoJSON` `MultiLineString`.
    MultiLineString,

    /// Build a `GeoJSON` `Polygon`.
    Polygon,

    /// Build a `GeoJSON` `MultiPolygon`.
    MultiPolygon,

    /// Parse as a date and time.
    DateTime,

    /// Parse as a date without a time.
    Date,

    /// Parse as a time without a date.
    Time,
}

impl Transformation {
    /// What this transformation asks a geometry to become, or `None` when it names no geometry at
    /// all.
    ///
    /// Both the load-time check of an attribute's `geometry` block and the extraction stage's
    /// dispatch read the target from here, so the two can never disagree about what a
    /// transformation names.
    #[must_use]
    pub const fn geometry_target(self) -> Option<GeometryTarget> {
        match self {
            Transformation::Geometry => Some(GeometryTarget::Preserve),
            Transformation::Point => Some(GeometryTarget::Coerce(GeometryKind::Point)),
            Transformation::MultiPoint => Some(GeometryTarget::Coerce(GeometryKind::MultiPoint)),
            Transformation::LineString => Some(GeometryTarget::Coerce(GeometryKind::LineString)),
            Transformation::MultiLineString => Some(GeometryTarget::Coerce(GeometryKind::MultiLineString)),
            Transformation::Polygon => Some(GeometryTarget::Coerce(GeometryKind::Polygon)),
            Transformation::MultiPolygon => Some(GeometryTarget::Coerce(GeometryKind::MultiPolygon)),
            Transformation::Boolean
            | Transformation::Integer
            | Transformation::Float
            | Transformation::String
            | Transformation::Array
            | Transformation::Object
            | Transformation::DateTime
            | Transformation::Date
            | Transformation::Time => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::transformation::Transformation;
    use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};

    #[test]
    fn transformations_deserialize_from_their_lowercase_tokens() {
        assert_eq!(serde_json::from_str::<Transformation>(r#""multipoint""#).unwrap(), Transformation::MultiPoint);
        assert_eq!(serde_json::from_str::<Transformation>(r#""datetime""#).unwrap(), Transformation::DateTime);
    }

    #[test]
    fn the_short_scalar_aliases_are_accepted() {
        assert_eq!(serde_json::from_str::<Transformation>(r#""bool""#).unwrap(), Transformation::Boolean);
        assert_eq!(serde_json::from_str::<Transformation>(r#""int""#).unwrap(), Transformation::Integer);
    }

    #[test]
    fn transformations_serialize_back_to_their_canonical_tokens() {
        assert_eq!(serde_json::to_string(&Transformation::Boolean).unwrap(), r#""boolean""#);
        assert_eq!(serde_json::to_string(&Transformation::LineString).unwrap(), r#""linestring""#);
    }

    #[test]
    fn the_generic_geometry_transformation_deserializes_from_its_token() {
        assert_eq!(serde_json::from_str::<Transformation>(r#""geometry""#).unwrap(), Transformation::Geometry);
    }

    #[test]
    fn each_geometry_transformation_names_the_type_it_targets() {
        assert_eq!(Transformation::Geometry.geometry_target(), Some(GeometryTarget::Preserve));
        assert_eq!(
            Transformation::MultiPolygon.geometry_target(),
            Some(GeometryTarget::Coerce(GeometryKind::MultiPolygon))
        );
    }

    #[test]
    fn a_scalar_transformation_names_no_geometry() {
        assert_eq!(Transformation::Float.geometry_target(), None);
        assert_eq!(Transformation::DateTime.geometry_target(), None);
    }

    #[test]
    fn an_unrecognised_transformation_is_rejected() {
        assert!(serde_json::from_str::<Transformation>(r#""geohash""#).is_err());
    }
}

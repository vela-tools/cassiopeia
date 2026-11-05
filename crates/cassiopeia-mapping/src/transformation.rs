use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter};

/// The conversion applied to an extracted source value before it becomes an NGSI-LD value.
///
/// The geometry variants name `GeoJSON` geometry types (RFC 7946 clause 3.1) and produce values
/// consumed by the corresponding NGSI-LD `GeoProperty` constructors. A transformation displays as
/// the canonical token a mapping writes for it, so a load error can quote it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumIter)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
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

    /// Read text as an absolute IRI (RFC 3987 clause 2.2) and write its normal form.
    ///
    /// Only the normalizations that keep the IRI equivalent apply: the syntax-based ones of RFC 3986
    /// clause 6.2.2 and, for `http` and `https`, the scheme-based ones of clause 6.2.3. Text that
    /// is no absolute IRI, a relative reference included, is refused, so the attribute is dropped
    /// with a warning.
    Iri,

    /// Read text as a URN (RFC 8141 clause 2) and write its normal form, with the `urn:` scheme and
    /// the namespace identifier lowercased.
    ///
    /// Text that is no URN is refused, so the attribute is dropped with a warning.
    Urn,

    /// Collect every source part into one JSON array.
    ///
    /// A part that is already an array contributes its elements, and so does text that reads as a
    /// JSON array, the way every value arrives from a CSV source; any other part contributes itself,
    /// so text that is no JSON array becomes one element. A null or blank part contributes nothing,
    /// and so does a blank text element of an array; no elements left means no value.
    Array,

    /// Keep a JSON object, reading one from text written as a JSON object.
    ///
    /// A value that is no object, and text that does not read as one, are refused, so the attribute
    /// is dropped with a warning. Blank text and an empty object resolve to no value.
    Object,

    /// Read text as the JSON value it is written as: an object, an array, a number, a boolean, or a
    /// string.
    ///
    /// Text that is not JSON is refused, so the attribute is dropped with a warning, and blank text
    /// resolves to no value. A value that is not text is already structured and is kept as it is.
    Json,

    /// Accept an already-formed `GeoJSON` geometry, inferring the type from the value.
    ///
    /// Unlike the type-specific geometry transformations below, which build a geometry of a fixed
    /// type from raw coordinates, this keeps the geometry the source already carries (a `GeoJSON`
    /// geometry object, RFC 7946 clause 3.1). NGSI-LD restrictions are enforced separately. This is
    /// the conversion a `GeoProperty` declaring no transformation uses (see
    /// [`Conversion::resolve`](crate::conversion::Conversion::resolve)).
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
            | Transformation::Iri
            | Transformation::Urn
            | Transformation::Array
            | Transformation::Object
            | Transformation::Json
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
    use pretty_assertions::assert_eq;

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
    fn a_transformation_displays_as_the_canonical_token_it_serializes_to() {
        for transformation in [
            Transformation::Integer,
            Transformation::MultiLineString,
            Transformation::DateTime,
            Transformation::Geometry,
        ] {
            assert_eq!(
                serde_json::to_value(transformation).unwrap(),
                serde_json::Value::String(transformation.to_string())
            );
        }
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
    fn the_json_transformation_round_trips_through_its_token() {
        assert_eq!(serde_json::from_str::<Transformation>(r#""json""#).unwrap(), Transformation::Json);
        assert_eq!(serde_json::to_string(&Transformation::Json).unwrap(), r#""json""#);
    }

    #[test]
    fn the_json_transformation_names_no_geometry() {
        assert_eq!(Transformation::Json.geometry_target(), None);
    }

    #[test]
    fn iri_and_urn_deserialize_from_their_tokens() {
        assert_eq!(serde_json::from_str::<Transformation>(r#""iri""#).unwrap(), Transformation::Iri);
        assert_eq!(serde_json::from_str::<Transformation>(r#""urn""#).unwrap(), Transformation::Urn);
    }

    #[test]
    fn iri_and_urn_serialize_and_display_as_their_tokens() {
        for (transformation, token) in [(Transformation::Iri, "iri"), (Transformation::Urn, "urn")] {
            assert_eq!(serde_json::to_string(&transformation).unwrap(), format!(r#""{token}""#));
            assert_eq!(transformation.to_string(), token);
            assert_eq!(transformation.geometry_target(), None);
        }
    }

    #[test]
    fn an_unrecognised_transformation_is_rejected() {
        assert!(serde_json::from_str::<Transformation>(r#""geohash""#).is_err());
    }
}

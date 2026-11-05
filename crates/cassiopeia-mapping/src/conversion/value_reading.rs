use crate::transformation::Transformation;
use cassiopeia_geometry::target::GeometryTarget;

/// Which temporal NGSI-LD value a reading produces (ETSI GS CIM 009 v1.9.1 clause 4.6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemporalReading {
    /// A date and time.
    DateTime,
    /// A date without a time.
    Date,
    /// A time without a date.
    Time,
}

/// How one value read from a source, or assembled from nested `mappings`, becomes a single NGSI-LD
/// value: every transformation but `array`, which gathers several values into one list instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueReading {
    /// A boolean, from a boolean, the text that spells one, or a number read as its truthiness.
    Boolean,
    /// A 64-bit signed integer.
    Integer,
    /// A finite double-precision number.
    Float,
    /// Text: text as it is, and any other value as its compact JSON text.
    Text,
    /// A date, a time, or both.
    Temporal(TemporalReading),
    /// A JSON object, or the object its JSON text spells.
    Object,
    /// The JSON value text spells, and any value that is not text as it is.
    Json,
    /// A `GeoJSON` geometry (RFC 7946 clause 3.1), kept as the source carries it or built as the type
    /// the target names.
    Geometry(GeometryTarget),
}

impl ValueReading {
    /// The reading a transformation names, or `None` for `array`, which reads no single value.
    #[must_use]
    pub const fn of(transformation: Transformation) -> Option<ValueReading> {
        match transformation {
            Transformation::Boolean => Some(ValueReading::Boolean),
            Transformation::Integer => Some(ValueReading::Integer),
            Transformation::Float => Some(ValueReading::Float),
            Transformation::String => Some(ValueReading::Text),
            Transformation::Object => Some(ValueReading::Object),
            Transformation::Json => Some(ValueReading::Json),
            Transformation::DateTime => Some(ValueReading::Temporal(TemporalReading::DateTime)),
            Transformation::Date => Some(ValueReading::Temporal(TemporalReading::Date)),
            Transformation::Time => Some(ValueReading::Temporal(TemporalReading::Time)),
            Transformation::Array => None,
            Transformation::Geometry
            | Transformation::Point
            | Transformation::MultiPoint
            | Transformation::LineString
            | Transformation::MultiLineString
            | Transformation::Polygon
            | Transformation::MultiPolygon => match transformation.geometry_target() {
                Some(target) => Some(ValueReading::Geometry(target)),
                None => None,
            },
        }
    }

    /// Whether this reading can turn a JSON object into a value.
    ///
    /// An object keeps itself under `object` and `json`, becomes its compact JSON text under
    /// `string`, and is read as the geometry it spells under a geometry reading. No object is a
    /// boolean, a number, or a date-time, so those readings never apply to one, and an attribute
    /// assembling its value from nested `mappings`, whose value is always an object, cannot declare
    /// them.
    #[must_use]
    pub const fn applies_to_object(self) -> bool {
        match self {
            ValueReading::Text | ValueReading::Object | ValueReading::Json | ValueReading::Geometry(_) => true,
            ValueReading::Boolean | ValueReading::Integer | ValueReading::Float | ValueReading::Temporal(_) => false,
        }
    }

    /// What this reading asks a geometry to become, or `None` when it reads no geometry.
    #[must_use]
    pub const fn geometry_target(self) -> Option<GeometryTarget> {
        match self {
            ValueReading::Geometry(target) => Some(target),
            ValueReading::Boolean
            | ValueReading::Integer
            | ValueReading::Float
            | ValueReading::Text
            | ValueReading::Temporal(_)
            | ValueReading::Object
            | ValueReading::Json => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        conversion::value_reading::{TemporalReading, ValueReading},
        transformation::Transformation,
    };
    use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn each_transformation_but_array_names_its_reading() {
        assert_eq!(ValueReading::of(Transformation::String), Some(ValueReading::Text));
        assert_eq!(ValueReading::of(Transformation::Date), Some(ValueReading::Temporal(TemporalReading::Date)));
        assert_eq!(
            ValueReading::of(Transformation::Geometry),
            Some(ValueReading::Geometry(GeometryTarget::Preserve))
        );
        assert_eq!(
            ValueReading::of(Transformation::Polygon),
            Some(ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Polygon)))
        );
        assert_eq!(ValueReading::of(Transformation::Array), None);
    }

    #[test]
    fn a_reading_targets_the_geometry_its_transformation_names() {
        for transformation in Transformation::iter().filter(|transformation| *transformation != Transformation::Array) {
            assert_eq!(
                ValueReading::of(transformation).and_then(ValueReading::geometry_target),
                transformation.geometry_target(),
                "{transformation}"
            );
        }
    }

    #[test]
    fn only_the_text_container_and_geometry_readings_apply_to_an_object() {
        for (transformation, applies) in [
            (Transformation::String, true),
            (Transformation::Object, true),
            (Transformation::Json, true),
            (Transformation::Point, true),
            (Transformation::Boolean, false),
            (Transformation::Integer, false),
            (Transformation::Float, false),
            (Transformation::DateTime, false),
            (Transformation::Time, false),
        ] {
            assert_eq!(
                ValueReading::of(transformation).map(ValueReading::applies_to_object),
                Some(applies),
                "{transformation}"
            );
        }
    }
}

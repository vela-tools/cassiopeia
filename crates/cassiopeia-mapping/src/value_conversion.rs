use crate::transformation::Transformation;
use cassiopeia_geometry::target::GeometryTarget;
use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;

/// How an attribute declaration's source values become its NGSI-LD value.
///
/// A declaration that names a `transformation` converts through exactly that transformation. One
/// that names none converts through the default its NGSI-LD attribute kind implies, which
/// [`ValueConversion::default_for`] decides. Keeping a value as it was read is not a transformation a
/// mapping can name: it exists only as the default of a kind whose value is raw JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueConversion {
    /// Convert through the named transformation.
    Transform(Transformation),

    /// Keep the value read from the source as it is: no coercion, no stringification, and no
    /// filtering by JSON type.
    Verbatim,
}

impl ValueConversion {
    /// The conversion an attribute of `kind` uses when its declaration names no transformation.
    ///
    /// A `GeoProperty`'s value shall be a `GeoJSON` geometry (ETSI GS CIM 009 v1.9.1 clause 4.7.1, and
    /// Table 5.2.7-1 of clause 5.2.7 types its `value` as a JSON object "as mandated by clause 4.7"),
    /// so it converts through [`Transformation::Geometry`]: the geometry the source carries, as an
    /// object or as its JSON text, is kept as read under the attribute's `geometry` policy. A
    /// declared transformation that reads no geometry is refused for a `GeoProperty` when the
    /// mapping loads, since it could never yield the geometry the value has to be.
    ///
    /// A `ListProperty`'s `valueList` is an ordered array (clause 4.5.21.2), so it collects its
    /// source values with [`Transformation::Array`]: an array read from the source becomes the list
    /// itself rather than the JSON text of it.
    ///
    /// A `JsonProperty`'s `json` member holds raw JSON that is never interpreted (clause 4.5.24.2
    /// names "a raw JSON object (or array of objects)", and Table 5.2.38-1 of clause 5.2.38 types
    /// it as `JSON` with no restriction), so the value is kept as read. [`Transformation::Object`]
    /// would wrongly drop an array, and [`Transformation::String`] would turn the JSON into text.
    ///
    /// Every other kind keeps its source value as text, which is what a declaration naming a source
    /// and no conversion has always produced.
    #[must_use]
    pub const fn default_for(kind: NgsiLdAttributeKind) -> ValueConversion {
        match kind {
            NgsiLdAttributeKind::GeoProperty => ValueConversion::Transform(Transformation::Geometry),
            NgsiLdAttributeKind::ListProperty => ValueConversion::Transform(Transformation::Array),
            NgsiLdAttributeKind::JsonProperty => ValueConversion::Verbatim,
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::Relationship
            | NgsiLdAttributeKind::ListRelationship
            | NgsiLdAttributeKind::LanguageProperty
            | NgsiLdAttributeKind::VocabProperty => ValueConversion::Transform(Transformation::String),
        }
    }

    /// What this conversion asks a geometry to become, or `None` when it reads no geometry at all.
    ///
    /// Only a transformation naming a geometry type reads one; keeping a value as read never does.
    #[must_use]
    pub const fn geometry_target(self) -> Option<GeometryTarget> {
        match self {
            ValueConversion::Transform(transformation) => transformation.geometry_target(),
            ValueConversion::Verbatim => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{transformation::Transformation, value_conversion::ValueConversion};
    use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;

    #[test]
    fn a_geo_property_defaults_to_the_geometry_transformation() {
        assert_eq!(
            ValueConversion::default_for(NgsiLdAttributeKind::GeoProperty),
            ValueConversion::Transform(Transformation::Geometry)
        );
    }

    #[test]
    fn a_list_property_defaults_to_the_array_transformation() {
        assert_eq!(
            ValueConversion::default_for(NgsiLdAttributeKind::ListProperty),
            ValueConversion::Transform(Transformation::Array)
        );
    }

    #[test]
    fn a_json_property_defaults_to_keeping_the_value_as_read() {
        assert_eq!(ValueConversion::default_for(NgsiLdAttributeKind::JsonProperty), ValueConversion::Verbatim);
    }

    #[test]
    fn every_other_kind_defaults_to_the_string_transformation() {
        for kind in [
            NgsiLdAttributeKind::Property,
            NgsiLdAttributeKind::Relationship,
            NgsiLdAttributeKind::ListRelationship,
            NgsiLdAttributeKind::LanguageProperty,
            NgsiLdAttributeKind::VocabProperty,
        ] {
            assert_eq!(
                ValueConversion::default_for(kind),
                ValueConversion::Transform(Transformation::String),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn the_geo_property_default_preserves_the_geometry_the_source_carries() {
        assert_eq!(
            ValueConversion::default_for(NgsiLdAttributeKind::GeoProperty).geometry_target(),
            Some(GeometryTarget::Preserve)
        );
    }

    #[test]
    fn a_geometry_type_transformation_targets_that_type() {
        assert_eq!(
            ValueConversion::Transform(Transformation::Polygon).geometry_target(),
            Some(GeometryTarget::Coerce(GeometryKind::Polygon))
        );
    }

    #[test]
    fn a_scalar_transformation_and_a_verbatim_value_read_no_geometry() {
        assert_eq!(ValueConversion::Transform(Transformation::String).geometry_target(), None);
        assert_eq!(ValueConversion::Verbatim.geometry_target(), None);
    }
}

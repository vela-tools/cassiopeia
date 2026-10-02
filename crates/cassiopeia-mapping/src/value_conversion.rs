use crate::transformation::Transformation;
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
    /// A `ListProperty`'s `valueList` is an ordered array (ETSI GS CIM 009 v1.9.1 clause 4.5.21.2), so it
    /// collects its source values with [`Transformation::Array`]: an array read from the source
    /// becomes the list itself rather than the JSON text of it.
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
            NgsiLdAttributeKind::ListProperty => ValueConversion::Transform(Transformation::Array),
            NgsiLdAttributeKind::JsonProperty => ValueConversion::Verbatim,
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::Relationship
            | NgsiLdAttributeKind::GeoProperty
            | NgsiLdAttributeKind::ListRelationship
            | NgsiLdAttributeKind::LanguageProperty
            | NgsiLdAttributeKind::VocabProperty => ValueConversion::Transform(Transformation::String),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{transformation::Transformation, value_conversion::ValueConversion};
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;

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
            NgsiLdAttributeKind::GeoProperty,
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
}

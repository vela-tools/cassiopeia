pub mod admitted_transformations;
pub mod conversion_refusal;
pub mod declaration_shape;
pub mod json_conversion;
pub mod list_conversion;
pub mod property_conversion;
pub mod value_reading;

use crate::{
    conversion::{
        admitted_transformations::AdmittedTransformations,
        conversion_refusal::ConversionRefusal,
        declaration_shape::{DeclarationShape, ValueOrigin},
        json_conversion::JsonConversion,
        list_conversion::ListConversion,
        property_conversion::PropertyConversion,
        value_reading::{IdentifierReading, ValueReading},
    },
    transformation::Transformation,
};
use cassiopeia_geometry::target::GeometryTarget;
use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
use strum::IntoEnumIterator;

/// How one declaration's source values, or the object its nested `mappings` assemble, become the
/// value its NGSI-LD attribute type holds.
///
/// One variant per type, each holding only the conversions that type admits, so a declaration
/// whose type and transformation could never produce a value has no conversion at all: it is
/// refused when the mapping loads (see [`Conversion::resolve`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Conversion {
    /// A `Property`'s value, any NGSI-LD Value (ETSI GS CIM 009 v1.9.1 clause 4.5.2).
    Property(PropertyConversion),
    /// A `GeoProperty`'s value, a `GeoJSON` geometry of one of the types clause 4.7.1 admits.
    GeoProperty(GeometryTarget),
    /// A `ListProperty`'s `valueList`, an ordered array of values (clause 4.5.21.2), gathered as
    /// the `array` transformation gathers them, its items kept as read or each read one way.
    ListProperty(ListConversion),
    /// A `JsonProperty`'s `json` member, raw JSON that is never interpreted (clause 4.5.24.2).
    JsonProperty(JsonConversion),
    /// A `LanguageProperty`'s `languageMap` (clause 4.5.18), read from one object of text keyed by
    /// language tag, or from its JSON text.
    LanguageProperty,
    /// One `languageMap` entry: one localized string.
    LanguageText,
    /// A `VocabProperty`'s `vocab` member (clause 4.5.20), read from text as the identifier the
    /// reading names.
    VocabProperty(IdentifierReading),
    /// A `Relationship` or `ListRelationship`, whose object the expander builds from the `source`
    /// and the `target`, so no conversion applies to it.
    Relationship,
}

impl Conversion {
    /// The conversion a declaration of `kind` and `shape` declaring `transformation`, or none,
    /// uses.
    ///
    /// Naming no transformation picks the type's default: text for a `Property` read from a
    /// `source` and the object as assembled for one built from nested `mappings`, the geometry as
    /// read for a `GeoProperty` (clause 4.7.1), the source gathered into a list for a
    /// `ListProperty` (clause 4.5.21.2), the value as read for a `JsonProperty` (clause
    /// 4.5.24.2), an object of tagged text for a `LanguageProperty`, and an IRI for a
    /// `VocabProperty`. A `languageMap` entry converts text, and the `LanguageProperty` holding the
    /// map takes no transformation of its own.
    ///
    /// # Errors
    /// Returns [`ConversionRefusal::Transformation`] for a transformation that can never produce
    /// the type's value from what the shape reads, and [`ConversionRefusal::Shape`] for a type that
    /// cannot be built from that shape at all.
    pub const fn resolve(kind: NgsiLdAttributeKind, transformation: Option<Transformation>, shape: DeclarationShape) -> Result<Conversion, ConversionRefusal> {
        match shape {
            DeclarationShape::Value(origin) => Conversion::resolve_value(kind, transformation, origin),
            DeclarationShape::LanguageMap => match (kind, transformation) {
                (NgsiLdAttributeKind::LanguageProperty, None) => Ok(Conversion::LanguageProperty),
                (NgsiLdAttributeKind::LanguageProperty, Some(transformation)) => Err(ConversionRefusal::Transformation(transformation)),
                (
                    NgsiLdAttributeKind::Property
                    | NgsiLdAttributeKind::GeoProperty
                    | NgsiLdAttributeKind::ListProperty
                    | NgsiLdAttributeKind::JsonProperty
                    | NgsiLdAttributeKind::VocabProperty
                    | NgsiLdAttributeKind::Relationship
                    | NgsiLdAttributeKind::ListRelationship,
                    _,
                ) => Err(ConversionRefusal::Shape),
            },
            DeclarationShape::LanguageEntry => match transformation {
                None | Some(Transformation::String) => Ok(Conversion::LanguageText),
                Some(transformation) => Err(ConversionRefusal::Transformation(transformation)),
            },
        }
    }

    /// The conversion of a declaration converting one value, read or assembled.
    const fn resolve_value(kind: NgsiLdAttributeKind, transformation: Option<Transformation>, origin: ValueOrigin) -> Result<Conversion, ConversionRefusal> {
        let refused = match transformation {
            Some(transformation) => ConversionRefusal::Transformation(transformation),
            None => ConversionRefusal::Shape,
        };

        match kind {
            NgsiLdAttributeKind::Property => match Conversion::property(transformation, origin) {
                Some(conversion) => Ok(Conversion::Property(conversion)),
                None => Err(refused),
            },
            NgsiLdAttributeKind::GeoProperty => match transformation {
                None => Ok(Conversion::GeoProperty(GeometryTarget::Preserve)),
                Some(transformation) => match transformation.geometry_target() {
                    Some(target) => Ok(Conversion::GeoProperty(target)),
                    None => Err(refused),
                },
            },
            NgsiLdAttributeKind::ListProperty => match Conversion::list(transformation, origin) {
                Some(conversion) => Ok(Conversion::ListProperty(conversion)),
                None => Err(refused),
            },
            NgsiLdAttributeKind::JsonProperty => match transformation {
                None => Ok(Conversion::JsonProperty(JsonConversion::Verbatim)),
                Some(Transformation::Json) => Ok(Conversion::JsonProperty(JsonConversion::Json)),
                Some(Transformation::Object) => Ok(Conversion::JsonProperty(JsonConversion::Object)),
                Some(Transformation::Array) => Ok(Conversion::JsonProperty(JsonConversion::Array)),
                Some(_) => Err(refused),
            },
            NgsiLdAttributeKind::LanguageProperty => match transformation {
                None | Some(Transformation::Object) => Ok(Conversion::LanguageProperty),
                Some(_) => Err(refused),
            },
            NgsiLdAttributeKind::VocabProperty => match (origin, transformation) {
                (ValueOrigin::Sourced, None | Some(Transformation::Iri)) => Ok(Conversion::VocabProperty(IdentifierReading::Iri)),
                (ValueOrigin::Sourced, Some(Transformation::Urn)) => Ok(Conversion::VocabProperty(IdentifierReading::Urn)),
                (ValueOrigin::Sourced, Some(_)) => Err(refused),
                (ValueOrigin::Assembled, _) => Err(ConversionRefusal::Shape),
            },
            NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship => match (origin, transformation) {
                (ValueOrigin::Sourced, None) => Ok(Conversion::Relationship),
                (ValueOrigin::Sourced, Some(_)) => Err(refused),
                (ValueOrigin::Assembled, _) => Err(ConversionRefusal::Shape),
            },
        }
    }

    /// A `Property`'s conversion, or `None` when the transformation cannot apply to what the
    /// origin reads.
    ///
    /// The object nested `mappings` assemble is kept as assembled when no transformation is named,
    /// since the mappings already spell out its structure field by field; a reading that never
    /// applies to an object cannot convert it.
    const fn property(transformation: Option<Transformation>, origin: ValueOrigin) -> Option<PropertyConversion> {
        let Some(transformation) = transformation else {
            return Some(PropertyConversion::One(match origin {
                ValueOrigin::Sourced => ValueReading::Text,
                ValueOrigin::Assembled => ValueReading::Object,
            }));
        };

        Conversion::declared_reading(transformation, origin)
    }

    /// A `ListProperty`'s conversion, or `None` when the transformation cannot apply to what the
    /// origin reads.
    ///
    /// Naming no transformation, or `array`, keeps every item as read; any other transformation
    /// reads each item as it would read one value (see [`Conversion::declared_reading`]).
    const fn list(transformation: Option<Transformation>, origin: ValueOrigin) -> Option<ListConversion> {
        let Some(transformation) = transformation else {
            return Some(ListConversion::AsRead);
        };

        match Conversion::declared_reading(transformation, origin) {
            Some(PropertyConversion::Array) => Some(ListConversion::AsRead),
            Some(PropertyConversion::One(reading)) => Some(ListConversion::Each(reading)),
            None => None,
        }
    }

    /// How a declared transformation reads what the origin reads, for a `Property` or a
    /// `ListProperty`: `array` gathers every part into one list, any other transformation reads one
    /// value, and `None` means the reading cannot apply.
    ///
    /// The value nested `mappings` assemble is always one object, so a reading that never applies to
    /// an object cannot read it.
    const fn declared_reading(transformation: Transformation, origin: ValueOrigin) -> Option<PropertyConversion> {
        match (ValueReading::of(transformation), origin) {
            (None, ValueOrigin::Sourced | ValueOrigin::Assembled) => Some(PropertyConversion::Array),
            (Some(reading), ValueOrigin::Sourced) => Some(PropertyConversion::One(reading)),
            (Some(reading), ValueOrigin::Assembled) => {
                if reading.applies_to_object() {
                    Some(PropertyConversion::One(reading))
                } else {
                    None
                }
            }
        }
    }

    /// Every transformation a declaration of `kind` and `shape` admits, in declaration order.
    ///
    /// Derived from [`Conversion::resolve`], so the list a load error offers can never disagree
    /// with what loads.
    #[must_use]
    pub fn admitted(kind: NgsiLdAttributeKind, shape: DeclarationShape) -> AdmittedTransformations {
        AdmittedTransformations::new(
            Transformation::iter()
                .filter(|transformation| Conversion::resolve(kind, Some(*transformation), shape).is_ok())
                .collect(),
        )
    }

    /// What this conversion asks a geometry to become, or `None` when it reads no geometry.
    #[must_use]
    pub const fn geometry_target(self) -> Option<GeometryTarget> {
        match self {
            Conversion::Property(PropertyConversion::One(reading)) | Conversion::ListProperty(ListConversion::Each(reading)) => reading.geometry_target(),
            Conversion::GeoProperty(target) => Some(target),
            Conversion::Property(PropertyConversion::Array)
            | Conversion::ListProperty(ListConversion::AsRead)
            | Conversion::JsonProperty(_)
            | Conversion::LanguageProperty
            | Conversion::LanguageText
            | Conversion::VocabProperty(_)
            | Conversion::Relationship => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        conversion::{
            Conversion,
            conversion_refusal::ConversionRefusal,
            declaration_shape::{DeclarationShape, ValueOrigin},
            json_conversion::JsonConversion,
            list_conversion::ListConversion,
            property_conversion::PropertyConversion,
            value_reading::{IdentifierReading, TemporalReading, ValueReading},
        },
        transformation::Transformation,
    };
    use cassiopeia_geometry::{geometry::GeometryKind, target::GeometryTarget};
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use strum::IntoEnumIterator;

    const SOURCED: DeclarationShape = DeclarationShape::Value(ValueOrigin::Sourced);
    const ASSEMBLED: DeclarationShape = DeclarationShape::Value(ValueOrigin::Assembled);

    /// Lists, for one type and shape, whether naming no transformation loads and which tokens do.
    ///
    /// Expands to one `(kind, shape, default_loads, admitted)` tuple per row, where `admitted` is the
    /// list of [`Transformation`] variants written in the row.
    macro_rules! admission_table {
        ($(($kind:ident, $shape:expr, default: $default:literal, [$($transformation:ident),* $(,)?])),* $(,)?) => {
            [$((NgsiLdAttributeKind::$kind, $shape, $default, vec![$(Transformation::$transformation),*])),*]
        };
    }

    #[test]
    fn every_combination_of_type_transformation_and_shape_is_admitted_exactly_as_the_table_states() {
        let table = admission_table![
            (Property, SOURCED, default: true, [
                Boolean, Integer, Float, String, Iri, Urn, Array, Object, Json, Geometry, Point, MultiPoint, LineString,
                MultiLineString, Polygon, MultiPolygon, DateTime, Date, Time,
            ]),
            (Property, ASSEMBLED, default: true, [
                String, Array, Object, Json, Geometry, Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon,
            ]),
            (Property, DeclarationShape::LanguageMap, default: false, []),
            (GeoProperty, SOURCED, default: true, [Geometry, Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon]),
            (GeoProperty, ASSEMBLED, default: true, [Geometry, Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon]),
            (GeoProperty, DeclarationShape::LanguageMap, default: false, []),
            (ListProperty, SOURCED, default: true, [
                Boolean, Integer, Float, String, Iri, Urn, Array, Object, Json, Geometry, Point, MultiPoint, LineString,
                MultiLineString, Polygon, MultiPolygon, DateTime, Date, Time,
            ]),
            (ListProperty, ASSEMBLED, default: true, [
                String, Array, Object, Json, Geometry, Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon,
            ]),
            (ListProperty, DeclarationShape::LanguageMap, default: false, []),
            (JsonProperty, SOURCED, default: true, [Array, Object, Json]),
            (JsonProperty, ASSEMBLED, default: true, [Array, Object, Json]),
            (JsonProperty, DeclarationShape::LanguageMap, default: false, []),
            (LanguageProperty, SOURCED, default: true, [Object]),
            (LanguageProperty, ASSEMBLED, default: true, [Object]),
            (LanguageProperty, DeclarationShape::LanguageMap, default: true, []),
            (VocabProperty, SOURCED, default: true, [Iri, Urn]),
            (VocabProperty, ASSEMBLED, default: false, []),
            (VocabProperty, DeclarationShape::LanguageMap, default: false, []),
            (Relationship, SOURCED, default: true, []),
            (Relationship, ASSEMBLED, default: false, []),
            (Relationship, DeclarationShape::LanguageMap, default: false, []),
            (ListRelationship, SOURCED, default: true, []),
            (ListRelationship, ASSEMBLED, default: false, []),
            (ListRelationship, DeclarationShape::LanguageMap, default: false, []),
            (Property, DeclarationShape::LanguageEntry, default: true, [String]),
            (GeoProperty, DeclarationShape::LanguageEntry, default: true, [String]),
            (ListProperty, DeclarationShape::LanguageEntry, default: true, [String]),
            (JsonProperty, DeclarationShape::LanguageEntry, default: true, [String]),
            (LanguageProperty, DeclarationShape::LanguageEntry, default: true, [String]),
            (VocabProperty, DeclarationShape::LanguageEntry, default: true, [String]),
            (Relationship, DeclarationShape::LanguageEntry, default: true, [String]),
            (ListRelationship, DeclarationShape::LanguageEntry, default: true, [String]),
        ];

        let rows = table.iter().map(|(kind, shape, _, _)| (*kind, *shape)).collect::<HashSet<_>>();
        assert_eq!(rows.len(), table.len(), "a type and shape is listed twice");
        for kind in NgsiLdAttributeKind::iter() {
            for shape in DeclarationShape::iter() {
                assert!(rows.contains(&(kind, shape)), "{kind} {shape} is not listed");
            }
        }
        for (kind, shape, default_loads, admitted) in table {
            assert_eq!(
                Conversion::resolve(kind, None, shape).is_ok(),
                default_loads,
                "{kind} {shape} with no transformation"
            );
            for transformation in Transformation::iter() {
                assert_eq!(
                    Conversion::resolve(kind, Some(transformation), shape).is_ok(),
                    admitted.contains(&transformation),
                    "{kind} {shape} under {transformation}"
                );
            }
        }
    }

    #[test]
    fn each_type_read_from_a_source_defaults_to_its_conversion_when_none_is_declared() {
        for (kind, expected) in [
            (NgsiLdAttributeKind::Property, Conversion::Property(PropertyConversion::One(ValueReading::Text))),
            (NgsiLdAttributeKind::GeoProperty, Conversion::GeoProperty(GeometryTarget::Preserve)),
            (NgsiLdAttributeKind::ListProperty, Conversion::ListProperty(ListConversion::AsRead)),
            (NgsiLdAttributeKind::JsonProperty, Conversion::JsonProperty(JsonConversion::Verbatim)),
            (NgsiLdAttributeKind::LanguageProperty, Conversion::LanguageProperty),
            (NgsiLdAttributeKind::VocabProperty, Conversion::VocabProperty(IdentifierReading::Iri)),
            (NgsiLdAttributeKind::Relationship, Conversion::Relationship),
            (NgsiLdAttributeKind::ListRelationship, Conversion::Relationship),
        ] {
            assert_eq!(Conversion::resolve(kind, None, SOURCED), Ok(expected), "{kind}");
        }
    }

    #[test]
    fn each_type_built_from_nested_mappings_defaults_to_its_assembled_conversion() {
        for (kind, expected) in [
            (
                NgsiLdAttributeKind::Property,
                Ok(Conversion::Property(PropertyConversion::One(ValueReading::Object))),
            ),
            (NgsiLdAttributeKind::GeoProperty, Ok(Conversion::GeoProperty(GeometryTarget::Preserve))),
            (NgsiLdAttributeKind::ListProperty, Ok(Conversion::ListProperty(ListConversion::AsRead))),
            (NgsiLdAttributeKind::JsonProperty, Ok(Conversion::JsonProperty(JsonConversion::Verbatim))),
            (NgsiLdAttributeKind::LanguageProperty, Ok(Conversion::LanguageProperty)),
            (NgsiLdAttributeKind::VocabProperty, Err(ConversionRefusal::Shape)),
            (NgsiLdAttributeKind::Relationship, Err(ConversionRefusal::Shape)),
        ] {
            assert_eq!(Conversion::resolve(kind, None, ASSEMBLED), expected, "{kind}");
        }
    }

    #[test]
    fn a_language_entry_converts_to_text_by_default_and_under_string() {
        assert_eq!(
            Conversion::resolve(NgsiLdAttributeKind::Property, None, DeclarationShape::LanguageEntry),
            Ok(Conversion::LanguageText)
        );
        assert_eq!(
            Conversion::resolve(NgsiLdAttributeKind::Property, Some(Transformation::String), DeclarationShape::LanguageEntry),
            Ok(Conversion::LanguageText)
        );
        assert_eq!(
            Conversion::resolve(NgsiLdAttributeKind::Property, Some(Transformation::Integer), DeclarationShape::LanguageEntry),
            Err(ConversionRefusal::Transformation(Transformation::Integer))
        );
    }

    #[test]
    fn each_admitted_transformation_resolves_to_its_conversion() {
        for (kind, transformation, expected) in [
            (
                NgsiLdAttributeKind::Property,
                Transformation::Integer,
                Conversion::Property(PropertyConversion::One(ValueReading::Integer)),
            ),
            (
                NgsiLdAttributeKind::Property,
                Transformation::Array,
                Conversion::Property(PropertyConversion::Array),
            ),
            (
                NgsiLdAttributeKind::Property,
                Transformation::DateTime,
                Conversion::Property(PropertyConversion::One(ValueReading::Temporal(TemporalReading::DateTime))),
            ),
            (
                NgsiLdAttributeKind::Property,
                Transformation::Geometry,
                Conversion::Property(PropertyConversion::One(ValueReading::Geometry(GeometryTarget::Preserve))),
            ),
            (
                NgsiLdAttributeKind::ListProperty,
                Transformation::Array,
                Conversion::ListProperty(ListConversion::AsRead),
            ),
            (
                NgsiLdAttributeKind::ListProperty,
                Transformation::Integer,
                Conversion::ListProperty(ListConversion::Each(ValueReading::Integer)),
            ),
            (
                NgsiLdAttributeKind::ListProperty,
                Transformation::Point,
                Conversion::ListProperty(ListConversion::Each(ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Point)))),
            ),
            (
                NgsiLdAttributeKind::JsonProperty,
                Transformation::Object,
                Conversion::JsonProperty(JsonConversion::Object),
            ),
            (
                NgsiLdAttributeKind::JsonProperty,
                Transformation::Json,
                Conversion::JsonProperty(JsonConversion::Json),
            ),
            (
                NgsiLdAttributeKind::JsonProperty,
                Transformation::Array,
                Conversion::JsonProperty(JsonConversion::Array),
            ),
            (NgsiLdAttributeKind::LanguageProperty, Transformation::Object, Conversion::LanguageProperty),
            (
                NgsiLdAttributeKind::VocabProperty,
                Transformation::Urn,
                Conversion::VocabProperty(IdentifierReading::Urn),
            ),
            (
                NgsiLdAttributeKind::Property,
                Transformation::Iri,
                Conversion::Property(PropertyConversion::One(ValueReading::Identifier(IdentifierReading::Iri))),
            ),
            (
                NgsiLdAttributeKind::ListProperty,
                Transformation::Urn,
                Conversion::ListProperty(ListConversion::Each(ValueReading::Identifier(IdentifierReading::Urn))),
            ),
            (
                NgsiLdAttributeKind::GeoProperty,
                Transformation::Point,
                Conversion::GeoProperty(GeometryTarget::Coerce(GeometryKind::Point)),
            ),
        ] {
            assert_eq!(
                Conversion::resolve(kind, Some(transformation), SOURCED),
                Ok(expected),
                "{kind} {transformation}"
            );
        }
    }

    #[test]
    fn a_type_that_cannot_be_built_from_its_shape_is_refused_whatever_the_transformation() {
        for transformation in [None, Some(Transformation::String), Some(Transformation::Object)] {
            assert_eq!(
                Conversion::resolve(NgsiLdAttributeKind::VocabProperty, transformation, ASSEMBLED),
                Err(ConversionRefusal::Shape),
                "{transformation:?}"
            );
            assert_eq!(
                Conversion::resolve(NgsiLdAttributeKind::Property, transformation, DeclarationShape::LanguageMap),
                Err(ConversionRefusal::Shape),
                "{transformation:?}"
            );
        }
    }

    #[test]
    fn the_admitted_transformations_of_a_type_are_listed_in_declaration_order() {
        for (kind, shape, expected) in [
            (
                NgsiLdAttributeKind::JsonProperty,
                SOURCED,
                vec![Transformation::Array, Transformation::Object, Transformation::Json],
            ),
            (NgsiLdAttributeKind::VocabProperty, SOURCED, vec![Transformation::Iri, Transformation::Urn]),
            (NgsiLdAttributeKind::Relationship, SOURCED, Vec::new()),
            (NgsiLdAttributeKind::LanguageProperty, DeclarationShape::LanguageMap, Vec::new()),
            (NgsiLdAttributeKind::Property, DeclarationShape::LanguageEntry, vec![Transformation::String]),
            (
                NgsiLdAttributeKind::Property,
                ASSEMBLED,
                vec![
                    Transformation::String,
                    Transformation::Array,
                    Transformation::Object,
                    Transformation::Json,
                    Transformation::Geometry,
                    Transformation::Point,
                    Transformation::MultiPoint,
                    Transformation::LineString,
                    Transformation::MultiLineString,
                    Transformation::Polygon,
                    Transformation::MultiPolygon,
                ],
            ),
        ] {
            assert_eq!(Conversion::admitted(kind, shape).as_slice(), expected.as_slice(), "{kind} {shape}");
        }
    }

    #[test]
    fn a_conversion_reading_a_geometry_names_its_target() {
        assert_eq!(
            Conversion::Property(PropertyConversion::One(ValueReading::Geometry(GeometryTarget::Preserve))).geometry_target(),
            Some(GeometryTarget::Preserve)
        );
        assert_eq!(
            Conversion::GeoProperty(GeometryTarget::Coerce(GeometryKind::Polygon)).geometry_target(),
            Some(GeometryTarget::Coerce(GeometryKind::Polygon))
        );
        assert_eq!(
            Conversion::ListProperty(ListConversion::Each(ValueReading::Geometry(GeometryTarget::Coerce(GeometryKind::Point)))).geometry_target(),
            Some(GeometryTarget::Coerce(GeometryKind::Point))
        );
        for conversion in [
            Conversion::Property(PropertyConversion::One(ValueReading::Integer)),
            Conversion::Property(PropertyConversion::Array),
            Conversion::ListProperty(ListConversion::AsRead),
            Conversion::ListProperty(ListConversion::Each(ValueReading::Integer)),
            Conversion::JsonProperty(JsonConversion::Verbatim),
            Conversion::LanguageProperty,
            Conversion::LanguageText,
            Conversion::VocabProperty(IdentifierReading::Iri),
            Conversion::Relationship,
        ] {
            assert_eq!(conversion.geometry_target(), None, "{conversion:?}");
        }
    }
}

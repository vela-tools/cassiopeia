use strum::{Display, EnumCount, EnumIter, EnumString};

/// The JSON Schema keyword a validation violation was raised by.
///
/// The names are spec-facing: JSON Schema (2020-12, section 10 onward) defines them in camelCase, so
/// they render exactly as the specification writes them and never in Cassiopeia's own kebab-case
/// convention. The vocabulary lives in the diagnostic crate rather than the validation stage because
/// it is what a diagnostic names; the mapping from a particular engine's error kinds onto it belongs
/// to the stage that drives that engine.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, EnumString, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "camelCase")]
pub enum SchemaKeyword {
    /// `additionalItems`.
    AdditionalItems,
    /// `additionalProperties`.
    AdditionalProperties,
    /// `anyOf`.
    AnyOf,
    /// `const`.
    Const,
    /// `contains`.
    Contains,
    /// `contentEncoding`.
    ContentEncoding,
    /// `contentMediaType`.
    ContentMediaType,
    /// `enum`.
    Enum,
    /// `exclusiveMaximum`.
    ExclusiveMaximum,
    /// `exclusiveMinimum`.
    ExclusiveMinimum,
    /// `format`.
    Format,
    /// `maxItems`.
    MaxItems,
    /// `maxLength`.
    MaxLength,
    /// `maxProperties`.
    MaxProperties,
    /// `maximum`.
    Maximum,
    /// `minItems`.
    MinItems,
    /// `minLength`.
    MinLength,
    /// `minProperties`.
    MinProperties,
    /// `minimum`.
    Minimum,
    /// `multipleOf`.
    MultipleOf,
    /// `not`.
    Not,
    /// `oneOf`.
    OneOf,
    /// `pattern`.
    Pattern,
    /// `propertyNames`.
    PropertyNames,
    /// `required`.
    Required,
    /// `type`.
    Type,
    /// `unevaluatedItems`.
    UnevaluatedItems,
    /// `unevaluatedProperties`.
    UnevaluatedProperties,
    /// `uniqueItems`.
    UniqueItems,
    /// A rule the engine raised that maps onto no single keyword: a `$ref` that would not resolve,
    /// a reference cycle, or a failure of the schema itself rather than of the instance.
    Schema,
}

#[cfg(test)]
mod tests {
    use crate::schema_keyword::SchemaKeyword;
    use std::collections::HashSet;
    use strum::{EnumCount, IntoEnumIterator};

    #[test]
    fn every_keyword_renders_a_distinct_json_schema_name() {
        let names: HashSet<String> = SchemaKeyword::iter().map(|keyword| keyword.to_string()).collect();

        assert_eq!(names.len(), SchemaKeyword::COUNT);
    }

    #[test]
    fn a_keyword_round_trips_through_its_specification_name() {
        for keyword in SchemaKeyword::iter() {
            assert_eq!(keyword.to_string().parse::<SchemaKeyword>(), Ok(keyword));
        }
    }

    #[test]
    fn keyword_names_use_the_specification_casing() {
        assert_eq!(SchemaKeyword::AdditionalProperties.to_string(), "additionalProperties");
        assert_eq!(SchemaKeyword::MinLength.to_string(), "minLength");
        assert_eq!(SchemaKeyword::Type.to_string(), "type");
        assert_eq!(SchemaKeyword::Enum.to_string(), "enum");
    }
}

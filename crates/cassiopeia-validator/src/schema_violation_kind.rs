use cassiopeia_diagnostic::schema_keyword::SchemaKeyword;
use derive_more::{AsRef, Display, From};
use jsonschema::error::{TypeKind, ValidationErrorKind};
use serde_json::Value;
use strum::{Display as StrumDisplay, EnumString};

/// One JSON object member name, as a schema or an instance spells it.
///
/// A member name is not free text: it addresses a place in a document. Keeping it as its own type
/// stops a required-property name from being confused with a message, and makes a violation's
/// payload readable without re-parsing prose.
#[derive(AsRef, Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct MemberName(Box<str>);

impl MemberName {
    /// Builds a member name.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>) -> MemberName {
        MemberName(name.into())
    }

    /// The member name's text.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

/// One of the seven type names JSON Schema's `type` keyword admits.
///
/// Cassiopeia's own mirror of the validation engine's type vocabulary, so a reported violation
/// carries a value this workspace owns rather than a type from whichever engine produced it.
#[derive(Clone, Copy, Debug, EnumString, Eq, Hash, Ord, PartialEq, PartialOrd, StrumDisplay)]
#[strum(serialize_all = "lowercase")]
pub enum JsonTypeName {
    /// A JSON array.
    Array,
    /// A JSON boolean.
    Boolean,
    /// A JSON number with no fractional part.
    Integer,
    /// JSON `null`.
    Null,
    /// Any JSON number.
    Number,
    /// A JSON object.
    Object,
    /// A JSON string.
    String,
}

/// What a schema expected, and what it got, for one violation.
///
/// The engine's own error kind is not carried through: it borrows the instance, holds nested error
/// trees for the combinator keywords, and would drag a validation dependency into every layer that
/// merely reports. This is the subset Cassiopeia can act on, owned outright.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaViolationKind {
    /// A property the schema requires was absent.
    MissingProperty {
        /// The property that should have been present.
        property: MemberName,
    },
    /// Properties the schema does not allow were present.
    UnexpectedProperties {
        /// The properties the schema did not admit.
        properties: Vec<MemberName>,
    },
    /// The value's JSON type is not one the schema admits.
    WrongType {
        /// The types the schema would have accepted.
        expected: Vec<JsonTypeName>,
    },
    /// The value fell outside a bound the schema set, or is not the required constant.
    OutOfBounds {
        /// The bound or constant the schema declared.
        limit: Value,
    },
    /// The value is not one of an enumerated set.
    NotEnumerated {
        /// The values the schema enumerated.
        options: Value,
    },
    /// The value did not satisfy a syntactic constraint: a format, a pattern, or a media type.
    Malformed {
        /// The constraint the schema named.
        constraint: MemberName,
    },
    /// The value matched none of a set of alternative schemas, or more than the one `oneOf` allows.
    Alternatives {
        /// Whether too many alternatives matched, rather than none.
        matched: AlternativeMatch,
    },
    /// The rule that failed carries no payload beyond its own name.
    Unstructured,
}

/// How a set of alternative schemas was not satisfied.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AlternativeMatch {
    /// The value satisfied none of the alternatives.
    None,
    /// The value satisfied more than the one `oneOf` permits.
    TooMany,
}

/// Maps one validation-engine error kind onto the keyword that raised it and the payload Cassiopeia
/// keeps.
///
/// The engine already names the keyword, so that half is taken from it verbatim and only re-typed;
/// a keyword this vocabulary does not model (a custom rule, a `$ref` that would not resolve)
/// reports as [`SchemaKeyword::Schema`], which is what it is: a failure of the schema rather than of
/// the instance.
#[must_use]
pub fn violation_of(kind: &ValidationErrorKind) -> (SchemaKeyword, SchemaViolationKind) {
    (keyword_of(kind), payload_of(kind))
}

/// Re-types the keyword the engine named.
fn keyword_of(kind: &ValidationErrorKind) -> SchemaKeyword {
    kind.keyword().parse().unwrap_or(SchemaKeyword::Schema)
}

/// Extracts the payload Cassiopeia keeps for one error kind.
fn payload_of(kind: &ValidationErrorKind) -> SchemaViolationKind {
    match kind {
        ValidationErrorKind::Required { property } => SchemaViolationKind::MissingProperty {
            property: MemberName::new(property.as_str().unwrap_or_default()),
        },
        ValidationErrorKind::AdditionalProperties { unexpected }
        | ValidationErrorKind::UnevaluatedProperties { unexpected }
        | ValidationErrorKind::UnevaluatedItems { unexpected } => SchemaViolationKind::UnexpectedProperties {
            properties: unexpected.iter().map(|property| MemberName::new(property.as_str())).collect(),
        },
        ValidationErrorKind::Type { kind } => SchemaViolationKind::WrongType { expected: type_names(kind) },
        ValidationErrorKind::Constant { expected_value } => SchemaViolationKind::OutOfBounds { limit: expected_value.clone() },
        ValidationErrorKind::ExclusiveMaximum { limit }
        | ValidationErrorKind::ExclusiveMinimum { limit }
        | ValidationErrorKind::Maximum { limit }
        | ValidationErrorKind::Minimum { limit } => SchemaViolationKind::OutOfBounds { limit: limit.clone() },
        ValidationErrorKind::MaxItems { limit }
        | ValidationErrorKind::MaxLength { limit }
        | ValidationErrorKind::MaxProperties { limit }
        | ValidationErrorKind::MinItems { limit }
        | ValidationErrorKind::MinLength { limit }
        | ValidationErrorKind::MinProperties { limit } => SchemaViolationKind::OutOfBounds { limit: Value::from(*limit) },
        ValidationErrorKind::MultipleOf { multiple_of } => SchemaViolationKind::OutOfBounds {
            limit: Value::from(*multiple_of),
        },
        ValidationErrorKind::Enum { options } => SchemaViolationKind::NotEnumerated { options: options.clone() },
        ValidationErrorKind::Format { format } => SchemaViolationKind::Malformed {
            constraint: MemberName::new(format.as_str()),
        },
        ValidationErrorKind::Pattern { pattern } => SchemaViolationKind::Malformed {
            constraint: MemberName::new(pattern.as_str()),
        },
        ValidationErrorKind::ContentEncoding { content_encoding } => SchemaViolationKind::Malformed {
            constraint: MemberName::new(content_encoding.as_str()),
        },
        ValidationErrorKind::ContentMediaType { content_media_type } => SchemaViolationKind::Malformed {
            constraint: MemberName::new(content_media_type.as_str()),
        },
        ValidationErrorKind::AnyOf { .. } | ValidationErrorKind::OneOfNotValid { .. } => SchemaViolationKind::Alternatives {
            matched: AlternativeMatch::None,
        },
        ValidationErrorKind::OneOfMultipleValid { .. } => SchemaViolationKind::Alternatives {
            matched: AlternativeMatch::TooMany,
        },
        ValidationErrorKind::AdditionalItems { .. }
        | ValidationErrorKind::BacktrackLimitExceeded { .. }
        | ValidationErrorKind::RegexEngineFailure { .. }
        | ValidationErrorKind::Contains
        | ValidationErrorKind::Custom { .. }
        | ValidationErrorKind::FalseSchema
        | ValidationErrorKind::FromUtf8 { .. }
        | ValidationErrorKind::Not { .. }
        | ValidationErrorKind::PropertyNames { .. }
        | ValidationErrorKind::UniqueItems
        | ValidationErrorKind::Referencing(_) => SchemaViolationKind::Unstructured,
    }
}

/// Re-types the engine's type set, dropping anything outside JSON's seven type names.
fn type_names(kind: &TypeKind) -> Vec<JsonTypeName> {
    match kind {
        TypeKind::Single(single) => single.as_str().parse().into_iter().collect(),
        TypeKind::Multiple(set) => set.iter().filter_map(|single| single.as_str().parse().ok()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_violation_kind::{AlternativeMatch, JsonTypeName, MemberName, SchemaViolationKind, violation_of};
    use cassiopeia_diagnostic::schema_keyword::SchemaKeyword;
    use jsonschema::Validator;
    use serde_json::{Value, json};

    /// The first violation a schema raises for an instance, as Cassiopeia keeps it.
    fn first(schema: &Value, instance: &Value) -> (SchemaKeyword, SchemaViolationKind) {
        let validator = Validator::new(schema).expect("a compilable schema");
        let error = validator.iter_errors(instance).next().expect("a violation");
        violation_of(error.kind())
    }

    #[test]
    fn a_missing_required_property_names_the_property() {
        let (keyword, kind) = first(&json!({"type": "object", "required": ["temperature"]}), &json!({}));

        assert_eq!(keyword, SchemaKeyword::Required);
        assert_eq!(
            kind,
            SchemaViolationKind::MissingProperty {
                property: MemberName::new("temperature")
            }
        );
    }

    #[test]
    fn a_wrong_type_names_the_admitted_types() {
        let (keyword, kind) = first(&json!({"type": "number"}), &json!("hot"));

        assert_eq!(keyword, SchemaKeyword::Type);
        assert_eq!(
            kind,
            SchemaViolationKind::WrongType {
                expected: vec![JsonTypeName::Number]
            }
        );
    }

    #[test]
    fn a_multi_type_violation_keeps_every_admitted_type() {
        let (_, kind) = first(&json!({"type": ["number", "boolean"]}), &json!("hot"));

        let SchemaViolationKind::WrongType { expected } = kind else {
            panic!("expected a type violation");
        };
        assert_eq!(expected.len(), 2);
        assert!(expected.contains(&JsonTypeName::Number));
        assert!(expected.contains(&JsonTypeName::Boolean));
    }

    #[test]
    fn a_bound_violation_keeps_the_limit() {
        let (keyword, kind) = first(&json!({"type": "number", "minimum": 5}), &json!(1));

        assert_eq!(keyword, SchemaKeyword::Minimum);
        assert_eq!(kind, SchemaViolationKind::OutOfBounds { limit: json!(5) });
    }

    #[test]
    fn a_length_violation_keeps_the_limit_as_a_number() {
        let (keyword, kind) = first(&json!({"type": "string", "minLength": 3}), &json!("a"));

        assert_eq!(keyword, SchemaKeyword::MinLength);
        assert_eq!(kind, SchemaViolationKind::OutOfBounds { limit: json!(3) });
    }

    #[test]
    fn an_enumeration_violation_keeps_the_options() {
        let (keyword, kind) = first(&json!({"enum": ["ok", "bad"]}), &json!("other"));

        assert_eq!(keyword, SchemaKeyword::Enum);
        assert_eq!(kind, SchemaViolationKind::NotEnumerated { options: json!(["ok", "bad"]) });
    }

    #[test]
    fn an_unexpected_property_violation_names_every_offender() {
        let (keyword, kind) = first(
            &json!({"type": "object", "properties": {"a": {}}, "additionalProperties": false}),
            &json!({"a": 1, "b": 2}),
        );

        assert_eq!(keyword, SchemaKeyword::AdditionalProperties);
        assert_eq!(
            kind,
            SchemaViolationKind::UnexpectedProperties {
                properties: vec![MemberName::new("b")]
            }
        );
    }

    #[test]
    fn a_one_of_violation_says_no_alternative_matched() {
        let (keyword, kind) = first(&json!({"oneOf": [{"type": "number"}, {"type": "boolean"}]}), &json!("hot"));

        assert_eq!(keyword, SchemaKeyword::OneOf);
        assert_eq!(
            kind,
            SchemaViolationKind::Alternatives {
                matched: AlternativeMatch::None
            }
        );
    }

    #[test]
    fn a_pattern_violation_keeps_the_pattern() {
        let (keyword, kind) = first(&json!({"type": "string", "pattern": "^[a-z]+$"}), &json!("ABC"));

        assert_eq!(keyword, SchemaKeyword::Pattern);
        assert_eq!(
            kind,
            SchemaViolationKind::Malformed {
                constraint: MemberName::new("^[a-z]+$")
            }
        );
    }

    #[test]
    fn a_rule_with_no_payload_reports_as_unstructured() {
        let (keyword, kind) = first(&json!({"type": "array", "uniqueItems": true}), &json!([1, 1]));

        assert_eq!(keyword, SchemaKeyword::UniqueItems);
        assert_eq!(kind, SchemaViolationKind::Unstructured);
    }
}

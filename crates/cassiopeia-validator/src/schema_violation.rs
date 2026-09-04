use crate::schema_violation_kind::{SchemaViolationKind, violation_of};
use cassiopeia_diagnostic::{json_pointer::JsonPointer, schema_keyword::SchemaKeyword};
use cassiopeia_ngsi_ld::entity::{name::NameBuf, reserved_member::is_reserved_member};
use getset::{CopyGetters, Getters};
use jsonschema::ValidationError;

/// One way an entity broke its schema, kept structured.
///
/// Building one *moves* the engine's owned parts rather than copying them: the two locations are
/// `Arc<str>`-backed, so they cost a refcount bump, and the deep instance value is dropped because
/// the offending value already appears inside the rendered message.
#[derive(Clone, CopyGetters, Debug, Eq, Getters, PartialEq)]
pub struct SchemaViolation {
    /// Where inside the entity the violation sits.
    #[getset(get = "pub")]
    instance_path: JsonPointer,
    /// Where inside the schema the violated rule lives.
    #[getset(get = "pub")]
    schema_path: JsonPointer,
    /// The keyword that raised it.
    #[getset(get_copy = "pub")]
    keyword: SchemaKeyword,
    /// What the schema expected, and what it got.
    #[getset(get = "pub")]
    kind: SchemaViolationKind,
    /// The engine's rendered message, which names the offending value.
    message: Box<str>,
}

impl SchemaViolation {
    /// Takes one engine error apart into the violation Cassiopeia keeps.
    #[must_use]
    pub fn from_error(error: ValidationError<'_>) -> SchemaViolation {
        let message = error.to_string().into_boxed_str();
        let parts = error.into_parts();
        let (keyword, kind) = violation_of(&parts.kind);
        SchemaViolation {
            instance_path: JsonPointer::new(parts.instance_path.as_str()),
            schema_path: JsonPointer::new(parts.schema_path.as_str()),
            keyword,
            kind,
            message,
        }
    }

    /// The engine's rendered message.
    #[must_use]
    pub const fn message(&self) -> &str {
        &self.message
    }

    /// The attribute the violation sits under, when it sits under one.
    ///
    /// ETSI GS CIM 009 v1.9.1 clause 4.5.1 makes every attribute a top-level member of the entity
    /// alongside the reserved ones, so the first segment of the instance pointer names the attribute,
    /// unless the pointer addresses the entity itself, or one of the members the specification
    /// reserves.
    #[must_use]
    pub fn attribute(&self) -> Option<NameBuf> {
        let segment = self.instance_path.first_segment()?;
        if is_reserved_member(segment) {
            return None;
        }
        NameBuf::new(segment).ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        schema_violation::SchemaViolation,
        schema_violation_kind::{MemberName, SchemaViolationKind},
    };
    use cassiopeia_diagnostic::schema_keyword::SchemaKeyword;
    use jsonschema::Validator;
    use serde_json::{Value, json};

    /// Every violation a schema raises for an instance, as Cassiopeia keeps them.
    fn violations(schema: &Value, instance: &Value) -> Vec<SchemaViolation> {
        let validator = Validator::new(schema).expect("a compilable schema");
        validator.iter_errors(instance).map(SchemaViolation::from_error).collect()
    }

    #[test]
    fn a_violation_keeps_its_schema_path_and_the_offending_value_in_its_message() {
        let violations = violations(
            &json!({"type": "object", "properties": {"temperature": {"type": "number"}}}),
            &json!({"temperature": "hot"}),
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].schema_path().as_str(), "/properties/temperature/type");
        assert_eq!(violations[0].instance_path().as_str(), "/temperature");
        assert_eq!(violations[0].keyword(), SchemaKeyword::Type);
        assert!(violations[0].message().contains("hot"));
    }

    #[test]
    fn a_violation_under_an_attribute_resolves_that_attribute() {
        let violations = violations(
            &json!({"type": "object", "properties": {"temperature": {"type": "object", "required": ["value"]}}}),
            &json!({"temperature": {}}),
        );

        assert_eq!(violations[0].attribute().expect("an attribute").to_string(), "temperature");
    }

    #[test]
    fn a_violation_at_the_entity_root_resolves_no_attribute() {
        let violations = violations(&json!({"type": "object", "required": ["temperature"]}), &json!({}));

        assert!(violations[0].attribute().is_none());
        assert_eq!(
            violations[0].kind(),
            &SchemaViolationKind::MissingProperty {
                property: MemberName::new("temperature")
            }
        );
    }

    #[test]
    fn a_violation_on_a_reserved_member_resolves_no_attribute() {
        let violations = violations(&json!({"type": "object", "properties": {"type": {"type": "string"}}}), &json!({"type": 7}));

        assert_eq!(violations[0].instance_path().as_str(), "/type");
        assert!(violations[0].attribute().is_none());
    }
}

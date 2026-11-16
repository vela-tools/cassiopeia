use crate::{
    error::ValidatorError,
    report::ValidationReportEntry,
    schema_verdict::{DiagnosticsLevel, ValidationDiagnostics, ValidationOutcome},
    schema_violation::SchemaViolation,
    schema_violation_kind::MemberName,
    schema_violations::{MAX_VIOLATIONS_PER_ENTITY, SchemaViolations},
};
use cassiopeia_diagnostic::json_pointer::JsonPointer;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use jsonschema::{Uri, ValidationError, Validator as JsonSchemaValidator, error::ValidationErrorKind};
use serde_json::{Value, json};

/// One member a schema requires at an entity's top level that a checked entity lacks.
#[derive(Clone, Debug)]
struct CarriedMember {
    /// The member the schema requires.
    member: MemberName,
    /// The violation the checked entity's lack of it is.
    violation: SchemaViolation,
    /// The path of keywords the schema was evaluated along to the `required` keyword naming it, the
    /// `evaluationPath` of a report unit.
    evaluation_path: JsonPointer,
    /// The absolute location of that keyword, the `schemaLocation` of a report unit, when the schema
    /// has a base URI.
    schema_location: Option<Uri<String>>,
}

impl CarriedMember {
    /// Takes the engine's error for the missing `member` apart.
    fn from_error(member: MemberName, error: ValidationError<'_>) -> CarriedMember {
        let evaluation_path = JsonPointer::new(error.evaluation_path().as_str());
        // The violation consumes the engine's error, which the location would otherwise borrow from.
        let schema_location = error.absolute_keyword_location().cloned();
        CarriedMember {
            member,
            violation: SchemaViolation::from_error(error),
            evaluation_path,
            schema_location,
        }
    }

    /// The `schemaLocation` of the report unit naming this member: the keyword's absolute location,
    /// or its evaluation path for a schema without a base URI, as the engine writes it.
    fn schema_location(&self) -> &str {
        self.schema_location.as_ref().map_or(self.evaluation_path.as_str(), Uri::as_str)
    }
}

/// The members a schema requires at an entity's top level that one checked entity lacks, carried
/// with it to the stage that folds it, with the other entities of its id, into the entity a run
/// writes.
///
/// A member one of those entities lacks may be held by another, and the entity written holds it as
/// soon as one does. It is missing only when every one of them lacks it, which is decided once the
/// last of them has been folded.
#[derive(Clone, Debug, Default)]
pub struct CarriedMembers {
    /// One missing-property violation per member, in the order the engine raised them.
    members: Vec<CarriedMember>,
}

impl CarriedMembers {
    /// No members: what an entity whose required members are decided on it carries.
    #[must_use]
    pub const fn none() -> CarriedMembers {
        CarriedMembers { members: Vec::new() }
    }

    /// Splits the violations of one known-invalid entity into the members its schema requires at
    /// the top level that it lacks, and every other violation, capped.
    ///
    /// Every violation is read, since a missing member may follow any number of others; only the
    /// first violations past the cap are left unbuilt.
    pub(crate) fn split(validator: &JsonSchemaValidator, entity: &Value) -> (CarriedMembers, SchemaViolations) {
        let mut members = Vec::new();
        let mut others = Vec::new();
        for error in validator.iter_errors(entity) {
            match missing_member(&error) {
                Some(member) => members.push(CarriedMember::from_error(member, error)),
                None if others.len() <= MAX_VIOLATIONS_PER_ENTITY => others.push(SchemaViolation::from_error(error)),
                None => {}
            }
        }
        (CarriedMembers { members }, SchemaViolations::capped(others))
    }

    /// Whether the checked entity lacks no member its schema requires at the top level.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The members, in the order the engine raised them.
    pub fn members(&self) -> impl Iterator<Item = &MemberName> {
        self.members.iter().map(|carried| &carried.member)
    }

    /// Keeps only the members `other` lacks as well, since a member another entity of the same id
    /// holds is held by the entity they are folded into.
    pub fn retain_shared(&mut self, other: &CarriedMembers) {
        self.members.retain(|carried| other.members().any(|member| *member == carried.member));
    }

    /// The outcome of the entity a run writes, folded from entities that all lack these members,
    /// with the diagnostics `diagnostics` asks for: conformant when there are none, and otherwise
    /// nonconformant, one violation per member.
    #[must_use]
    pub fn into_outcome(self, entity: &NgsiLdEntity, diagnostics: DiagnosticsLevel) -> ValidationOutcome {
        if self.is_empty() {
            return ValidationOutcome::conformant();
        }
        let diagnostics = match diagnostics {
            DiagnosticsLevel::None => ValidationDiagnostics::None,
            DiagnosticsLevel::Errors => ValidationDiagnostics::Errors {
                error: Box::new(self.into_error(entity)),
            },
            DiagnosticsLevel::Report => {
                let entry = ValidationReportEntry {
                    // Report entries outlive the borrowed entity and therefore own their identity.
                    entity_id: entity.id.clone(),
                    entity_type: entity.entity_type.clone(),
                    evaluation: self.evaluation(),
                };
                ValidationDiagnostics::Report {
                    error: Box::new(self.into_error(entity)),
                    entry,
                }
            }
        };
        ValidationOutcome::nonconformant(diagnostics)
    }

    /// The failure of the entity a run writes, one violation per member.
    fn into_error(self, entity: &NgsiLdEntity) -> ValidatorError {
        ValidatorError::ValidationFailed {
            // Diagnostics must own their identity after the borrowed entity leaves validation.
            entity_id: entity.id.clone(),
            entity_type: entity.entity_type.clone(),
            violations: SchemaViolations::capped(self.members.into_iter().map(|carried| carried.violation).collect()),
        }
    }

    /// The members as a list-format evaluation, one unit per `required` keyword with the message of
    /// each of its members, in the shape the engine writes the units of the keyword.
    fn evaluation(&self) -> Value {
        let mut units: Vec<(&CarriedMember, Vec<&str>)> = Vec::new();
        for carried in &self.members {
            match units.iter_mut().find(|(first, _)| first.evaluation_path == carried.evaluation_path) {
                Some((_, messages)) => messages.push(carried.violation.message()),
                None => units.push((carried, vec![carried.violation.message()])),
            }
        }
        let details: Vec<Value> = units
            .into_iter()
            .map(|(first, messages)| {
                let errors = match messages.as_slice() {
                    [message] => json!(message),
                    many => json!(many),
                };
                json!({
                    "valid": false,
                    "evaluationPath": first.evaluation_path.as_str(),
                    "schemaLocation": first.schema_location(),
                    "instanceLocation": "",
                    "errors": { "required": errors },
                })
            })
            .collect();
        json!({ "valid": false, "details": details })
    }
}

/// The member an engine error says the entity lacks at its top level, or `None` for any other
/// error.
fn missing_member(error: &ValidationError<'_>) -> Option<MemberName> {
    if let ValidationErrorKind::Required { property } = error.kind()
        && error.instance_path().as_str().is_empty()
    {
        return property.as_str().map(MemberName::new);
    }
    None
}

/// Removes from a checked entity's list-format evaluation the units of the `required` keywords at
/// its top level, whose members are carried rather than decided on it.
///
/// A unit evaluates one keyword at one instance location, so a unit at the top level with `required`
/// errors names only members the entity lacks there, all of which are carried.
pub(crate) fn remove_carried_units(evaluation: &mut Value) {
    if let Some(details) = evaluation.get_mut("details").and_then(Value::as_array_mut) {
        details.retain(|unit| !is_carried_unit(unit));
    }
}

/// Whether a list-format unit holds `required` errors at the entity's top level.
fn is_carried_unit(unit: &Value) -> bool {
    unit.get("instanceLocation").and_then(Value::as_str) == Some("") && unit.get("errors").is_some_and(|errors| errors.get("required").is_some())
}

#[cfg(test)]
mod tests {
    use crate::{
        carried_members::{CarriedMembers, remove_carried_units},
        error::ValidatorError,
        schema_verdict::{DiagnosticsLevel, SchemaVerdict, ValidationDiagnostics},
        schema_violation_kind::MemberName,
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use jsonschema::Validator;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use urn_rs::Urn;

    /// A schema that requires `dateObserved` and `location` at the top level and `pm10` to be a
    /// number, as the Smart Data Models `AirQualityObserved` schema does.
    fn validator() -> Validator {
        jsonschema::options()
            .with_base_uri("http://localhost/")
            .build(&json!({
                "type": "object",
                "properties": {"pm10": {"type": "number"}, "location": {"type": "object", "required": ["type"]}},
                "required": ["id", "type", "dateObserved", "location"],
            }))
            .unwrap()
    }

    /// The members a record lacks, split from its other violations.
    fn split(record: &Value) -> CarriedMembers {
        let (carried, others) = CarriedMembers::split(&validator(), record);
        assert!(others.is_empty(), "{others}");
        carried
    }

    fn names(carried: &CarriedMembers) -> Vec<&str> {
        carried.members().map(MemberName::as_str).collect()
    }

    fn bus() -> NgsiLdEntity {
        NgsiLdEntity::new(
            "urn:ngsi-ld:AirQualityObserved:bus-12".parse::<Urn>().unwrap(),
            NameBuf::new("AirQualityObserved").unwrap(),
        )
    }

    /// The record of an Observation of the bus, which has no `location`.
    fn observation() -> Value {
        json!({"id": "urn:ngsi-ld:AirQualityObserved:bus-12", "type": "AirQualityObserved", "dateObserved": "2026-11-16T08:00:00Z", "pm10": 18.2})
    }

    /// The record of a `HistoricalLocation` of the bus, which has no `dateObserved`.
    fn historical_location() -> Value {
        json!({"id": "urn:ngsi-ld:AirQualityObserved:bus-12", "type": "AirQualityObserved", "location": {"type": "Point", "coordinates": [5.4797, 51.4433]}})
    }

    #[test]
    fn a_member_required_at_the_top_level_is_carried_and_any_other_violation_is_not() {
        let (carried, others) = CarriedMembers::split(
            &validator(),
            &json!({"id": "urn:ngsi-ld:AirQualityObserved:bus-12", "type": "AirQualityObserved", "pm10": "high", "location": {}}),
        );

        assert_eq!(names(&carried), ["dateObserved"]);
        assert_eq!(others.violations().len(), 2);
        assert_eq!(others.violations()[0].instance_path().as_str(), "/pm10");
        assert_eq!(others.violations()[1].instance_path().as_str(), "/location");
    }

    #[test]
    fn records_that_each_lack_a_different_member_fold_into_an_entity_that_lacks_none() {
        let mut missing = split(&observation());
        missing.retain_shared(&split(&historical_location()));

        assert!(missing.is_empty());
        assert_eq!(missing.into_outcome(&bus(), DiagnosticsLevel::Report).verdict(), SchemaVerdict::Conformant);
    }

    #[test]
    fn records_that_all_lack_one_member_fold_into_an_entity_that_violates_its_schema_once() {
        let mut missing = split(&observation());
        missing.retain_shared(&split(
            &json!({"id": "urn:ngsi-ld:AirQualityObserved:bus-12", "type": "AirQualityObserved", "pm10": 21.5}),
        ));

        assert_eq!(names(&missing), ["location"]);
        let outcome = missing.into_outcome(&bus(), DiagnosticsLevel::Errors);
        assert_eq!(outcome.verdict(), SchemaVerdict::Nonconformant);
        let ValidationDiagnostics::Errors { error } = outcome.into_diagnostics() else {
            panic!("expected error diagnostics");
        };
        let ValidatorError::ValidationFailed { violations, .. } = &*error else {
            panic!("expected a validation failure");
        };
        assert_eq!(violations.violations().len(), 1);
        assert_eq!(
            error.to_string(),
            r#"Entity urn:ngsi-ld:AirQualityObserved:bus-12 of type 'AirQualityObserved' does not conform to its schema: 1 violation(s), first at '': "location" is a required property"#
        );
    }

    #[test]
    fn the_report_entry_of_a_folded_entity_lists_each_missing_member_once_under_its_keyword() {
        let missing = split(&json!({"id": "urn:ngsi-ld:AirQualityObserved:bus-12", "type": "AirQualityObserved"}));

        let ValidationDiagnostics::Report { entry, .. } = missing.into_outcome(&bus(), DiagnosticsLevel::Report).into_diagnostics() else {
            panic!("expected report diagnostics");
        };

        assert_eq!(entry.entity_id.to_string(), "urn:ngsi-ld:AirQualityObserved:bus-12");
        assert_eq!(
            entry.evaluation,
            json!({
                "valid": false,
                "details": [{
                    "valid": false,
                    "evaluationPath": "/required",
                    "schemaLocation": "http://localhost/#/required",
                    "instanceLocation": "",
                    "errors": {"required": ["\"dateObserved\" is a required property", "\"location\" is a required property"]},
                }],
            })
        );
    }

    #[test]
    fn the_report_entry_of_one_missing_member_holds_its_message_alone() {
        let missing = split(&observation());

        let ValidationDiagnostics::Report { entry, .. } = missing.into_outcome(&bus(), DiagnosticsLevel::Report).into_diagnostics() else {
            panic!("expected report diagnostics");
        };

        assert_eq!(
            entry.evaluation["details"][0]["errors"],
            json!({"required": "\"location\" is a required property"})
        );
    }

    #[test]
    fn removing_the_carried_units_keeps_every_other_unit_of_an_evaluation() {
        let record =
            json!({"id": "urn:ngsi-ld:AirQualityObserved:bus-12", "type": "AirQualityObserved", "dateObserved": "2026-11-16T08:00:00Z", "pm10": "high"});
        let mut evaluation = serde_json::to_value(validator().evaluate(&record).list()).unwrap();

        remove_carried_units(&mut evaluation);

        let paths: Vec<&str> = evaluation["details"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|unit| unit.get("errors").is_some())
            .map(|unit| unit["evaluationPath"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["/properties/pm10/type"]);
    }

    #[test]
    fn no_members_conform() {
        assert_eq!(
            CarriedMembers::none().into_outcome(&bus(), DiagnosticsLevel::Errors).verdict(),
            SchemaVerdict::Conformant
        );
    }
}

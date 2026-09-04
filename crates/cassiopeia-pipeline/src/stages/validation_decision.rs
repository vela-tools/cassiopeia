use cassiopeia_manifest::output::validation_mode::ValidationMode;
use cassiopeia_validator::{
    error::Result as ValidatorResult,
    schema_verdict::{DiagnosticsLevel, SchemaVerdict, ValidationOutcome},
};

/// Why an entity was warned about rather than forwarded silently or refused.
///
/// The three are already distinguished by [`decide`]; naming them is what lets the stage group its
/// warnings without a second pass over the entity, which is the whole reason relaxed validation can
/// stay free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WarnReason {
    /// The entity's type has no schema, so it was never checked.
    SchemaAbsent,
    /// The entity was checked and did not conform.
    Nonconformant,
    /// A schema exists but could not be read, parsed, or compiled.
    SchemaUnusable,
}

/// What the validator stage does with one entity, given its verdict and the run's validation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Send the entity on and count it as processed.
    Forward,
    /// Send the entity on but count it only as a warning, not as processed.
    Warn(WarnReason),
    /// Emit an error signal and tear the stage down; the entity is not counted as processed.
    Abort,
}

/// Whether this run needs structured validation report entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reporting {
    /// No report was requested.
    Disabled,
    /// A report path was given.
    Enabled,
}

/// Chooses what the stage does with an entity from its verdict and the run's validation mode.
///
/// A missing schema is tolerated in every mode but the strict one; a schema-backed failure (whether
/// a nonconformance or a broken schema, the latter surfacing as an `Err`) is fatal in both fail-fast
/// modes and only a warning in the relaxed one.
pub(crate) const fn decide(mode: ValidationMode, verdict: &ValidatorResult<ValidationOutcome>) -> Decision {
    match verdict {
        Err(_error) => match mode {
            ValidationMode::Warn => Decision::Warn(WarnReason::SchemaUnusable),
            ValidationMode::FailWhenSchema | ValidationMode::Fail => Decision::Abort,
        },
        Ok(outcome) => match outcome.verdict() {
            SchemaVerdict::Conformant => Decision::Forward,
            SchemaVerdict::Absent => match mode {
                ValidationMode::Warn => Decision::Forward,
                ValidationMode::FailWhenSchema => Decision::Warn(WarnReason::SchemaAbsent),
                ValidationMode::Fail => Decision::Abort,
            },
            SchemaVerdict::Nonconformant => match mode {
                ValidationMode::Warn => Decision::Warn(WarnReason::Nonconformant),
                ValidationMode::FailWhenSchema | ValidationMode::Fail => Decision::Abort,
            },
        },
    }
}

/// Chooses the cheapest diagnostics that can satisfy the run's enforcement and reporting needs.
///
/// Relaxed validation with no report path asks for none, and that has to stay true: in that mode
/// every entity can be nonconformant, so a second pass per entity to collect violations would be
/// paid on the whole stream.
pub(crate) const fn diagnostics_level(mode: ValidationMode, reporting: Reporting) -> DiagnosticsLevel {
    match reporting {
        Reporting::Enabled => DiagnosticsLevel::Report,
        Reporting::Disabled => match mode {
            ValidationMode::Warn => DiagnosticsLevel::None,
            ValidationMode::FailWhenSchema | ValidationMode::Fail => DiagnosticsLevel::Errors,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::stages::validation_decision::{Decision, Reporting, WarnReason, decide, diagnostics_level};
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_manifest::output::validation_mode::ValidationMode;
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_validator::{
        error::{Result as ValidatorResult, ValidatorError},
        schema_validator::SchemaValidator,
        schema_validator_config::SchemaValidatorConfig,
        schema_verdict::{DiagnosticsLevel, ValidationDiagnostics, ValidationOutcome},
        schema_violations::SchemaViolations,
        validator::Validator,
    };
    use jsonschema::Validator as JsonSchemaValidator;
    use serde_json::json;
    use std::{collections::HashMap, fs};
    use tempfile::TempDir;
    use urn_rs::Urn;

    /// A nonconformant verdict carrying a stub failure.
    fn nonconformant() -> ValidationOutcome {
        let schema = JsonSchemaValidator::new(&json!({"type": "object", "required": ["temperature"]})).unwrap();
        ValidationOutcome::nonconformant(ValidationDiagnostics::Errors {
            error: Box::new(ValidatorError::ValidationFailed {
                entity_id: "urn:ngsi-ld:Sensor:1".parse().unwrap(),
                entity_type: NameBuf::new("Sensor").unwrap(),
                violations: SchemaViolations::collect(&schema, &json!({})),
            }),
        })
    }

    /// An infrastructure failure verdict, standing in for a broken or unusable schema.
    fn infra_error() -> ValidatorError {
        ValidatorError::SchemaFileIsNull {
            path: "/schemas/Sensor.json".into(),
        }
    }

    /// Every verdict a mode can face, paired with the decision each mode must render: relaxed,
    /// fail-when-schema, then strict.
    fn cases() -> Vec<(ValidatorResult<ValidationOutcome>, Decision, Decision, Decision)> {
        vec![
            (
                Ok(ValidationOutcome::absent()),
                Decision::Forward,
                Decision::Warn(WarnReason::SchemaAbsent),
                Decision::Abort,
            ),
            (Ok(ValidationOutcome::conformant()), Decision::Forward, Decision::Forward, Decision::Forward),
            (Ok(nonconformant()), Decision::Warn(WarnReason::Nonconformant), Decision::Abort, Decision::Abort),
            (Err(infra_error()), Decision::Warn(WarnReason::SchemaUnusable), Decision::Abort, Decision::Abort),
        ]
    }

    #[test]
    fn every_mode_and_verdict_pair_renders_the_matrix_decision() {
        for (verdict, warn, fail_when_schema, fail) in cases() {
            assert_eq!(decide(ValidationMode::Warn, &verdict), warn);
            assert_eq!(decide(ValidationMode::FailWhenSchema, &verdict), fail_when_schema);
            assert_eq!(decide(ValidationMode::Fail, &verdict), fail);
        }
    }

    #[test]
    fn a_missing_custom_schema_aborts_when_enforced_and_warns_when_relaxed() {
        // An explicitly requested schema that is absent surfaces as a `CustomSchemaMissing` `Err`, so
        // it routes through the same fatal path as any other schema failure without a matrix change.
        let verdict: ValidatorResult<ValidationOutcome> = Err(ValidatorError::CustomSchemaMissing {
            entity_type: NameBuf::new("ExoPlanet").unwrap(),
            path: "/schemas/ExoPlanet.json".into(),
        });

        assert_eq!(decide(ValidationMode::Warn, &verdict), Decision::Warn(WarnReason::SchemaUnusable));
        assert_eq!(decide(ValidationMode::FailWhenSchema, &verdict), Decision::Abort);
        assert_eq!(decide(ValidationMode::Fail, &verdict), Decision::Abort);
    }

    #[test]
    fn relaxed_validation_without_a_report_requests_no_diagnostics() {
        assert_eq!(diagnostics_level(ValidationMode::Warn, Reporting::Disabled), DiagnosticsLevel::None);
    }

    #[test]
    fn enforcing_validation_without_a_report_requests_error_messages() {
        for mode in [ValidationMode::FailWhenSchema, ValidationMode::Fail] {
            assert_eq!(diagnostics_level(mode, Reporting::Disabled), DiagnosticsLevel::Errors);
        }
    }

    #[test]
    fn a_requested_report_always_requests_structured_diagnostics() {
        for mode in [ValidationMode::Warn, ValidationMode::FailWhenSchema, ValidationMode::Fail] {
            assert_eq!(diagnostics_level(mode, Reporting::Enabled), DiagnosticsLevel::Report);
        }
    }

    #[test]
    fn a_conformant_entity_carries_no_diagnostics_at_any_level() {
        // The cheapness contract: nothing below the `is_valid` fast path runs for a conformant
        // entity, whatever diagnostics the caller asked for.
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Sensor.json"), r#"{"type": "object"}"#).unwrap();
        let validator = SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: directory.path().to_path_buf(),
            repositories: HashMap::new(),
            custom_schemas: HashMap::new(),
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
        });
        let entity = NgsiLdEntity::new("urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(), NameBuf::new("Sensor").unwrap());

        for level in [DiagnosticsLevel::None, DiagnosticsLevel::Errors, DiagnosticsLevel::Report] {
            let outcome = validator.check(&entity, level).unwrap();
            assert!(matches!(outcome.into_diagnostics(), ValidationDiagnostics::None));
        }
    }
}

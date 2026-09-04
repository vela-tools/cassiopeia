use crate::{error::PipelineError, stages::validation_report::ReportCollector};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_validator::{
    error::{Result as ValidatorResult, ValidatorError},
    schema_verdict::{SchemaVerdict, ValidationDiagnostics, ValidationOutcome},
};

/// Builds the pipeline error that aborts the run, recording the report entry for a nonconformance
/// first so the report written at finalize captures the offender.
///
/// A [`SchemaVerdict::Conformant`] never reaches this path (a conformant entity is always forwarded);
/// it is folded into the missing-schema arm only to keep the match exhaustive.
pub(crate) fn abort_error(verdict: ValidatorResult<ValidationOutcome>, entity: &NgsiLdEntity, collector: Option<&mut ReportCollector>) -> PipelineError {
    match verdict {
        Err(error) => PipelineError::Validation(error),
        Ok(outcome) => match outcome.verdict() {
            SchemaVerdict::Nonconformant => nonconformance_error(outcome.into_diagnostics(), entity, collector),
            SchemaVerdict::Absent | SchemaVerdict::Conformant => PipelineError::ValidationSchemaMissing {
                // Pipeline errors own their context after the borrowed entity is dropped.
                entity_id: entity.id.clone(),
                entity_type: entity.entity_type.clone(),
            },
        },
    }
}

/// Converts diagnostics for an aborting nonconformance into the pipeline error and records its
/// structured entry when reporting is enabled.
fn nonconformance_error(diagnostics: ValidationDiagnostics, entity: &NgsiLdEntity, collector: Option<&mut ReportCollector>) -> PipelineError {
    match diagnostics {
        ValidationDiagnostics::Errors { error } => PipelineError::Validation(*error),
        ValidationDiagnostics::Report { error, entry } => {
            if let Some(collector) = collector {
                collector.record(entry);
            }
            PipelineError::Validation(*error)
        }
        ValidationDiagnostics::None => PipelineError::Validation(ValidatorError::MissingDiagnostics {
            // This error crosses the stage boundary and therefore owns the entity's identity.
            entity_id: entity.id.clone(),
            entity_type: entity.entity_type.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::PipelineError, stages::validation_abort::abort_error};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_validator::{
        error::ValidatorError,
        schema_verdict::{ValidationDiagnostics, ValidationOutcome},
    };
    use urn_rs::Urn;

    fn entity() -> NgsiLdEntity {
        NgsiLdEntity::new("urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(), NameBuf::new("Sensor").unwrap())
    }

    #[test]
    fn an_absent_schema_abort_names_the_failing_entity() {
        let error = abort_error(Ok(ValidationOutcome::absent()), &entity(), None);

        let PipelineError::ValidationSchemaMissing { entity_id, entity_type } = error else {
            panic!("expected a missing-schema abort");
        };
        assert_eq!(entity_id.to_string(), "urn:ngsi-ld:Sensor:1");
        assert_eq!(entity_type.to_string(), "Sensor");
    }

    #[test]
    fn a_nonconformance_without_diagnostics_names_the_failing_entity() {
        let error = abort_error(Ok(ValidationOutcome::nonconformant(ValidationDiagnostics::None)), &entity(), None);

        let PipelineError::Validation(ValidatorError::MissingDiagnostics { entity_id, entity_type }) = error else {
            panic!("expected a missing-diagnostics failure");
        };
        assert_eq!(entity_id.to_string(), "urn:ngsi-ld:Sensor:1");
        assert_eq!(entity_type.to_string(), "Sensor");
    }
}

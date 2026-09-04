use crate::error::Error;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};

/// The code the run's fatal failure is published under.
///
/// A pipeline failure carries its own subsystem code, so the reason table names the same thing the
/// live diagnostics did. Everything else is a failure of the command surface itself: configuration,
/// a handler, the reporter, for which no narrower subsystem exists.
#[must_use]
pub fn failure_code(error: &Error) -> DiagnosticCode {
    match error {
        Error::Pipeline(error) => DiagnosticCode::from(error),
        Error::Config(_) | Error::Cli(_) | Error::Manifest(_) | Error::Reporter(_) | Error::CliValidation(_) | Error::ThreadPool { .. } | Error::CtrlC(_) => {
            DiagnosticCode::Run(RunCode::Failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::Error, failure_code::failure_code};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode, schema_code::SchemaCode};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_pipeline::{error::PipelineError, pipeline_stage::PipelineStage};
    use urn_rs::Urn;

    #[test]
    fn a_pipeline_failure_keeps_its_own_subsystem_code() {
        let error = Error::Pipeline(PipelineError::ValidationSchemaMissing {
            entity_id: "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
        });

        assert_eq!(failure_code(&error), DiagnosticCode::Schema(SchemaCode::Absent));
    }

    #[test]
    fn a_stage_panic_reports_as_a_run_failure() {
        let error = Error::Pipeline(PipelineError::StagePanic { stage: PipelineStage::Writer });

        assert_eq!(failure_code(&error), DiagnosticCode::Run(RunCode::StagePanic));
    }
}

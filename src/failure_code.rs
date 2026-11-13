use crate::error::Error;
use cassiopeia_cli::error::CliError;
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode};

/// The code the run's fatal failure is published under.
///
/// A pipeline failure carries its own subsystem code, so the reason table names the same thing the
/// live diagnostics did, and a cursor command fails with the code a run's unusable cursor does.
/// Everything else is a failure of the command surface itself: configuration, a handler, the
/// reporter, for which no narrower subsystem exists.
#[must_use]
pub fn failure_code(error: &Error) -> DiagnosticCode {
    match error {
        Error::Pipeline(error) => DiagnosticCode::from(error),
        Error::Cli(CliError::Cursor(_)) => DiagnosticCode::Run(RunCode::CursorUnusable),
        Error::Config(_) | Error::Cli(_) | Error::Manifest(_) | Error::Reporter(_) | Error::CliValidation(_) | Error::ThreadPool { .. } | Error::CtrlC(_) => {
            DiagnosticCode::Run(RunCode::Failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::Error, failure_code::failure_code};
    use cassiopeia_cli::error::CliError;
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, run_code::RunCode, schema_code::SchemaCode};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_pipeline::{error::PipelineError, pipeline_stage::PipelineStage};
    use cassiopeia_sensorthings::cursor_name::CursorName;
    use cassiopeia_sensorthings_cursor::cursor_store_error::CursorStoreError;
    use pretty_assertions::assert_eq;
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

    #[test]
    fn a_cursor_command_that_cannot_use_its_cursor_reports_as_a_run_s_unusable_cursor() {
        let cursor = CursorName::try_from("rivm-air".to_owned()).unwrap();
        let error = Error::Cli(CliError::Cursor(CursorStoreError::InUse { cursor }));

        assert_eq!(failure_code(&error), DiagnosticCode::Run(RunCode::CursorUnusable));
    }

    #[test]
    fn any_other_handler_failure_reports_as_a_failed_run() {
        let error = Error::Cli(CliError::GetFilenameFromDocument);

        assert_eq!(failure_code(&error), DiagnosticCode::Run(RunCode::Failed));
    }
}

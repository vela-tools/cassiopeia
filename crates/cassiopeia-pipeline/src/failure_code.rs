use crate::{error::PipelineError, stages::coded_error::CodedError};
use cassiopeia_collector::error::CollectorError;
use cassiopeia_diagnostic::code::{
    broker_code::BrokerCode,
    diagnostic_code::DiagnosticCode,
    ingest_code::IngestCode,
    run_code::RunCode,
    schema_code::SchemaCode,
};
use cassiopeia_ingestor::error::IngestorError;
use cassiopeia_profiler::error::ProfilerError;
use cassiopeia_validator::error::ValidatorError;
use cassiopeia_writer::error::WriterError;

/// The code a fatal pipeline failure is published under.
///
/// This is where the workspace's error types meet the diagnostic vocabulary. Every match is
/// exhaustive, so a new failure variant is a compile error here rather than a failure that quietly
/// reports under the wrong name.
impl From<&PipelineError> for DiagnosticCode {
    fn from(error: &PipelineError) -> DiagnosticCode {
        match error {
            PipelineError::Collector(error) => collector_code(error).into(),
            PipelineError::Profiler(error) => profiler_code(error).into(),
            PipelineError::Ingestor(error) => ingestor_code(error).into(),
            // A resolution failure is the composition's, not one stage's: the fragment resolver and
            // its two backing stores fail as one subsystem.
            PipelineError::Resolver(_) | PipelineError::EntityStore(_) | PipelineError::RelationshipStore(_) => RunCode::ResolutionFailed.into(),
            PipelineError::Expander(error) => error.code().into(),
            PipelineError::Extraction(error) => error.code().into(),
            PipelineError::Transformation(error) => error.code().into(),
            PipelineError::Writer(error) => writer_code(error),
            PipelineError::Validation(error) => validator_code(error).into(),
            PipelineError::ValidationSchemaMissing { .. } => SchemaCode::Absent.into(),
            PipelineError::SchemaFetch { .. } => SchemaCode::FetchFailed.into(),
            PipelineError::SeriesRequiresTemporalOperation => RunCode::Misconfigured.into(),
            PipelineError::Mapping(_) => RunCode::MappingUnusable.into(),
            PipelineError::StagePanic { .. } => RunCode::StagePanic.into(),
        }
    }
}

/// Why a source could not be collected.
const fn collector_code(error: &CollectorError) -> IngestCode {
    match error {
        CollectorError::ChannelClosed => IngestCode::StreamClosed,
        CollectorError::ClientInit { .. }
        | CollectorError::Http { .. }
        | CollectorError::HttpStatus { .. }
        | CollectorError::Io { .. }
        | CollectorError::InvalidFileExtension { .. } => IngestCode::SourceUnavailable,
    }
}

/// Why a payload could not be profiled or routed.
const fn profiler_code(error: &ProfilerError) -> IngestCode {
    match error {
        ProfilerError::Profiling(_) => IngestCode::FormatUndetected,
        ProfilerError::ChannelClosed => IngestCode::StreamClosed,
        ProfilerError::NoRoute { .. } => IngestCode::Unroutable,
    }
}

/// Why a payload's records could not be read.
const fn ingestor_code(error: &IngestorError) -> IngestCode {
    match error {
        IngestorError::ChannelClosed => IngestCode::StreamClosed,
        IngestorError::CannotBeStreamedTwice
        | IngestorError::Io { .. }
        | IngestorError::Csv(_)
        | IngestorError::GeoJson(_)
        | IngestorError::Grib(_)
        | IngestorError::Json(_)
        | IngestorError::Kml(_)
        | IngestorError::Shapefile(_)
        | IngestorError::Xml(_) => IngestCode::RecordsUnreadable,
    }
}

/// Why the destination could not accept the run's entities.
///
/// A broker failure keeps its own broker code so the reason table names the same thing the live
/// diagnostics did; a file-side failure is the output's, with no more specific subsystem to name.
fn writer_code(error: &WriterError) -> DiagnosticCode {
    match error {
        WriterError::BrokerRequest { .. } => BrokerCode::TransportFailed.into(),
        WriterError::BrokerRejectedBatch { .. } => BrokerCode::BatchRejected.into(),
        WriterError::BrokerOpaqueStatus { .. } => BrokerCode::BatchRejectedOpaque.into(),
        WriterError::BrokerMultiStatusUnreadable { .. } => BrokerCode::BatchUnreadable.into(),
        WriterError::BrokerBatchUnaccounted { .. } => BrokerCode::BatchUnaccounted.into(),
        WriterError::BrokerEntityDropped { .. } => BrokerCode::EntityDropped.into(),
        WriterError::BrokerWorkerPoolGone { .. } => BrokerCode::WorkerPoolGone.into(),
        WriterError::BrokerWorkerPanicked { .. } => BrokerCode::WorkerPanicked.into(),
        WriterError::BrokerDeliveryFailed { .. } => BrokerCode::DeliveryFailed.into(),
        WriterError::AtomicSpool(_) => BrokerCode::SpoolFailed.into(),
        WriterError::SimdSerialization { .. } => BrokerCode::SerializationFailed.into(),
        WriterError::ClientInit(_) | WriterError::InvalidBrokerUrl { .. } | WriterError::Io(_) => RunCode::OutputFailed.into(),
    }
}

/// Why a schema check could not be completed.
const fn validator_code(error: &ValidatorError) -> SchemaCode {
    match error {
        ValidatorError::ValidationFailed { .. } => SchemaCode::Nonconformant,
        ValidatorError::Io(_)
        | ValidatorError::SchemaFileIsNull { .. }
        | ValidatorError::ParseSchemaFile { .. }
        | ValidatorError::CompileValidator { .. }
        | ValidatorError::SerializeEntity { .. }
        | ValidatorError::SerializeEvaluation { .. }
        | ValidatorError::MissingDiagnostics { .. }
        | ValidatorError::CustomSchemaMissing { .. } => SchemaCode::Unusable,
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::PipelineError, pipeline_stage::PipelineStage};
    use cassiopeia_common::{collection::CollectionName, format::DataFormat};
    use cassiopeia_diagnostic::code::{
        diagnostic_code::DiagnosticCode,
        expander_code::ExpanderCode,
        ingest_code::IngestCode,
        run_code::RunCode,
        schema_code::SchemaCode,
    };
    use cassiopeia_expander::error::ExpanderError;
    use cassiopeia_ir::payload_origin::PayloadOrigin;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_profiler::error::ProfilerError;
    use std::path::PathBuf;
    use urn_rs::Urn;

    #[test]
    fn a_stage_panic_is_a_run_failure() {
        let error = PipelineError::StagePanic { stage: PipelineStage::Writer };

        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Run(RunCode::StagePanic));
    }

    #[test]
    fn an_unroutable_payload_names_its_file_and_is_an_ingest_failure() {
        let error = PipelineError::Profiler(ProfilerError::NoRoute {
            format: DataFormat::Csv,
            origin: PayloadOrigin::File(PathBuf::from("/data/stations.csv")),
        });

        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Ingest(IngestCode::Unroutable));
        assert!(error.to_string().contains("/data/stations.csv"));
    }

    #[test]
    fn an_unroutable_in_memory_payload_says_so_rather_than_naming_a_path() {
        let error = PipelineError::Profiler(ProfilerError::NoRoute {
            format: DataFormat::Csv,
            origin: PayloadOrigin::Memory,
        });

        assert!(error.to_string().contains("in-memory"));
    }

    #[test]
    fn an_expansion_failure_keeps_its_own_reason() {
        let error = PipelineError::Expander(ExpanderError::UnmatchedCollection(CollectionName::from("Camera")));

        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Expander(ExpanderCode::CollectionUnmatched));
    }

    #[test]
    fn a_missing_schema_under_strict_validation_is_an_absent_schema() {
        let error = PipelineError::ValidationSchemaMissing {
            entity_id: "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
        };

        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Schema(SchemaCode::Absent));
    }
}

use crate::pipeline_stage::PipelineStage;
use cassiopeia_collector::error::CollectorError;
use cassiopeia_expander::error::ExpanderError;
use cassiopeia_extractor::error::ExtractionError;
use cassiopeia_ingestor::error::IngestorError;
use cassiopeia_mapping::error::MappingError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use cassiopeia_profiler::error::ProfilerError;
use cassiopeia_resolver::{entity_store::error::EntityStoreError, error::ResolverError, relationship_store::error::RelationshipStoreError};
use cassiopeia_transformer::error::TransformationError;
use cassiopeia_validator::error::ValidatorError;
use cassiopeia_writer::error::WriterError;
use std::result;
use thiserror::Error;
use url::Url;
use urn_rs::Urn;

/// A failure that aborts a pipeline run.
///
/// Each stage's own error is carried typed through a `#[from]` variant; the remaining variants cover
/// failures that belong to the composition itself: a mapping that will not load, a worker thread
/// that panicked, or the resolver sink reporting it could not finish.
///
/// The passthrough variants are `transparent`: a wrapper that both prefixes the inner message and
/// chains to the inner error prints that message twice, once as the headline and once as its own
/// first cause. Which stage failed is what the diagnostic *code* says, so the message does not have
/// to repeat it.
#[derive(Debug, Error)]
pub enum PipelineError {
    /// A source could not be collected.
    #[error(transparent)]
    Collector(#[from] CollectorError),

    /// A payload's format could not be profiled or routed.
    #[error(transparent)]
    Profiler(#[from] ProfilerError),

    /// A payload could not be ingested into records.
    #[error(transparent)]
    Ingestor(#[from] IngestorError),

    /// A fragment could not be resolved.
    #[error(transparent)]
    Resolver(#[from] ResolverError),

    /// The entity store failed.
    #[error(transparent)]
    EntityStore(#[from] EntityStoreError),

    /// The relationship store failed.
    #[error(transparent)]
    RelationshipStore(#[from] RelationshipStoreError),

    /// A record could not be expanded into fragments.
    #[error(transparent)]
    Expander(#[from] ExpanderError),

    /// An entity's attributes could not be extracted.
    #[error(transparent)]
    Extraction(#[from] ExtractionError),

    /// An entity could not be transformed into NGSI-LD.
    #[error(transparent)]
    Transformation(#[from] TransformationError),

    /// An entity could not be written to the destination.
    #[error(transparent)]
    Writer(#[from] WriterError),

    /// An entity failed schema validation, or its schema was broken, under a fail-fast mode.
    #[error(transparent)]
    Validation(#[from] ValidatorError),

    /// An entity type had no schema file under the strict validation mode, which forbids made-up
    /// data models.
    #[error("No schema found for entity {entity_id} of type '{entity_type}' under strict validation")]
    ValidationSchemaMissing {
        /// The entity whose type had no schema.
        entity_id: Urn,
        /// The entity type whose schema was absent.
        entity_type: NameBuf,
    },

    /// A series temporal representation was configured against a Context Broker whose operation is not
    /// `temporal`. A folded `EntityTemporal` only belongs at the broker's `/temporal/entities`
    /// endpoint (ETSI GS CIM 009 v1.9.1 clause 5.6.11).
    #[error("output.temporal.representation \"series\" against a Context Broker requires operation \"temporal\"")]
    SeriesRequiresTemporalOperation,

    /// A remote custom validation schema could not be fetched at run setup.
    #[error("Failed to fetch the custom validation schema from '{url}'")]
    SchemaFetch {
        /// The URL that could not be fetched.
        url: Url,
        /// The underlying HTTP or I/O failure from the download helper. Boxed so this variant does
        /// not enlarge every `Result` in the crate past the large-error threshold.
        #[source]
        source: Box<CollectorError>,
    },

    /// A mapping file could not be read or parsed.
    #[error(transparent)]
    Mapping(#[from] MappingError),

    /// A stage's worker thread panicked, so the run cannot continue.
    #[error("The {stage} stage panicked")]
    StagePanic {
        /// The stage whose worker thread panicked.
        stage: PipelineStage,
    },
}

/// The result type used across the pipeline crate.
pub type Result<T, E = PipelineError> = result::Result<T, E>;

use crate::{error::Result, pipeline::Pipeline, pipeline_stage::PipelineStage};
use cassiopeia_common::{
    broker_atomicity::BrokerAtomicity,
    broker_operation::{BrokerOperation, BrokerOperationKind},
    store_kind::StoreKind,
};
use cassiopeia_manifest::output::{context_delivery::ContextDelivery as ManifestContextDelivery, destination::Destination};
use cassiopeia_mapping::template::resolver::TemplateResolver;
use cassiopeia_ngsi_ld::{
    data_model::DataModelRepository,
    entity::{context::ContextSource, name::NameBuf},
};
use cassiopeia_reporter::reporter::ProgressStage;
use cassiopeia_resolver::{
    entity_store::{
        dashmap_latest_store::DashMapLatestEntityStore,
        dashmap_series_store::DashMapSeriesEntityStore,
        redb_latest_store::RedbLatestEntityStore,
        redb_series_store::RedbSeriesEntityStore,
        store::EntityStore,
    },
    fragment_resolver::FragmentResolver,
    relationship_store::{dashmap_store::DashMapRelationshipStore, redb_store::RedbRelationshipStore, store::RelationshipStore},
};
use cassiopeia_validator::{schema_validator::SchemaValidator, schema_validator_config::SchemaValidatorConfig};
use cassiopeia_writer::{
    broker::{atomic_writer::AtomicWriter, broker_writer::BrokerWriter, config::BrokerWriterConfig},
    context_delivery::ContextDelivery,
    file::file_writer::{FileWriter, FileWriterConfig, Parallelism},
    writer::Writer,
};
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf};

impl Pipeline {
    /// Whether the run needs the series entity store, which retains every observation of an id.
    ///
    /// The series store is chosen when the output requests a series representation, or when a Context
    /// Broker destination uses `operation: "temporal"`: each single-instance observation is then
    /// streamed to `/temporal/entities`, a minimal-memory per-observation path with no fold stage.
    pub(crate) const fn uses_series_store(&self) -> bool {
        self.output.series_representation
            || matches!(
                &self.output.destination,
                Destination::ContextBroker {
                    operation: BrokerOperationKind::Temporal,
                    ..
                }
            )
    }

    /// Builds the entity and relationship stores the configuration selects, wired to the shared
    /// template resolver. The entity store's memory model (current-state or series) is chosen from
    /// the run's temporal target.
    pub(crate) fn create_resolver(&self, template_resolver: TemplateResolver) -> Result<FragmentResolver> {
        let series = self.uses_series_store();
        let entity_store: Box<dyn EntityStore> = match (self.config.entity_store, series) {
            (StoreKind::DashMap, false) => Box::new(DashMapLatestEntityStore::new()),
            (StoreKind::DashMap, true) => Box::new(DashMapSeriesEntityStore::new()),
            (StoreKind::Redb, false) => Box::new(RedbLatestEntityStore::new()?),
            (StoreKind::Redb, true) => Box::new(RedbSeriesEntityStore::new()?),
        };
        let relationship_store: Box<dyn RelationshipStore> = match self.config.relationship_store {
            StoreKind::DashMap => Box::new(DashMapRelationshipStore::new()),
            StoreKind::Redb => Box::new(RedbRelationshipStore::new()?),
        };
        Ok(FragmentResolver::new(entity_store, relationship_store, template_resolver))
    }

    /// Builds the writer for the manifest's destination, taking representation and skip-null from the
    /// manifest output when set, otherwise from the NGSI-LD domain defaults (Normalized, skip nulls).
    pub(crate) fn create_writer(&self, context: ContextSource) -> Result<Box<dyn Writer>> {
        let representation = self.output.representation.unwrap_or_default();
        let skip_null = self.output.skip_null.unwrap_or_default();

        let writer: Box<dyn Writer> = match &self.output.destination {
            Destination::File { directory, framing } => {
                let output_dir = directory.clone().unwrap_or_else(|| PathBuf::from("."));
                Box::new(FileWriter::new(FileWriterConfig {
                    output_dir,
                    representation,
                    skip_null,
                    parallelism: Parallelism::Parallel,
                    context,
                    // A file keeps its `@context` in the body; there is no header to carry a link.
                    context_delivery: ContextDelivery::Body,
                    framing: *framing,
                    batch_size: NonZeroUsize::new(self.config.batch_size).unwrap_or(NonZeroUsize::MIN),
                })?)
            }
            Destination::ContextBroker {
                url,
                tenant,
                user_agent,
                headers,
                context_delivery,
                operation,
                upsert_mode,
                attribute_overwrite,
                atomicity,
            } => {
                // The manifest destination's user-agent wins; the build-time default fills in when
                // it names none.
                let user_agent = user_agent.as_ref().unwrap_or(&self.config.default_user_agent).clone();
                // The manifest carries the operation as a flat kind plus its two option enums; the
                // composition root reassembles the tight domain value.
                let operation = BrokerOperation::from_parts(*operation, *upsert_mode, *attribute_overwrite);
                let config = BrokerWriterConfig::new(url.clone(), user_agent, self.context.shutdown, self.context.reporter)
                    .with_representation(representation)
                    .with_skip_null(skip_null)
                    .with_context(context)
                    .with_context_delivery(map_context_delivery(*context_delivery))
                    .with_operation(operation)
                    .with_tenant(tenant.clone())
                    .with_headers(headers.clone())
                    .with_stage_id(PipelineStage::Writer.id());
                let broker = BrokerWriter::new(config)?;

                // Atomic delivery spools every entity and pushes only on a clean finish; streaming
                // pushes as entities arrive.
                match atomicity {
                    BrokerAtomicity::Streaming => Box::new(broker),
                    BrokerAtomicity::Atomic => Box::new(AtomicWriter::new(Box::new(broker))?),
                }
            }
        };

        // The series fold is a streaming post-validator stage, not a writer decorator. The writer
        // stack emits whatever the fold hands it, or, for a current-state run, whatever the
        // validator hands it directly.
        Ok(writer)
    }

    /// Builds the schema validator from the configured schemas folder and the run's resolved
    /// validation representation.
    ///
    /// The `repositories` map carries each entity type's publishing repository, taken from the
    /// run's qualified data models, so a catalog schema stored at `<folder>/<repository>/<Type>.json`
    /// is found rather than only a flat `<folder>/<Type>.json`. The `custom_schemas` map carries an
    /// explicit schema file for each type that requested one, overriding that convention.
    pub(crate) fn create_validator(&self, repositories: HashMap<NameBuf, DataModelRepository>, custom_schemas: HashMap<NameBuf, PathBuf>) -> SchemaValidator {
        SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: self.config.schemas_folder.clone(),
            repositories,
            custom_schemas,
            representation: self.output.validation_representation,
            skip_null: self.output.validation_skip_null,
        })
    }
}

/// Maps the manifest's context-delivery choice onto the writer's.
const fn map_context_delivery(delivery: ManifestContextDelivery) -> ContextDelivery {
    match delivery {
        ManifestContextDelivery::Body => ContextDelivery::Body,
        ManifestContextDelivery::LinkHeader => ContextDelivery::LinkHeader,
    }
}

#[cfg(test)]
mod tests {
    use crate::factories::map_context_delivery;
    use cassiopeia_manifest::output::context_delivery::ContextDelivery as ManifestContextDelivery;
    use cassiopeia_writer::context_delivery::ContextDelivery;

    #[test]
    fn context_delivery_maps_across_the_two_enums() {
        assert_eq!(map_context_delivery(ManifestContextDelivery::Body), ContextDelivery::Body);
        assert_eq!(map_context_delivery(ManifestContextDelivery::LinkHeader), ContextDelivery::LinkHeader);
    }
}

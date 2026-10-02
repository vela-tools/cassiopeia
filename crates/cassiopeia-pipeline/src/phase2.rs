use crate::{
    completion::drain_completion,
    controller::RunController,
    error::Result,
    pipeline::Pipeline,
    stages::{
        aggregator::spawn_aggregator_thread,
        assembler::spawn_assembler_thread,
        extractor::spawn_extractor_thread,
        stage_env::StageEnv,
        transformer::spawn_transformer_thread,
        validator::{ValidationSettings, spawn_validator_thread},
        writer::spawn_writer_thread,
    },
};
use cassiopeia_common::{stage::Stage, telemetry::run::RunTelemetry};
use cassiopeia_mapping::template::resolver::TemplateResolver;
use cassiopeia_resolver::{entity_source::EntitySource, fragment_resolver::FragmentResolver};
use cassiopeia_validator::schema_validator::SchemaValidator;
use cassiopeia_writer::writer::Writer;
use std::sync::Arc;

impl Pipeline {
    /// Extracts, transforms, validates, and writes resolved entities.
    ///
    /// # Errors
    ///
    /// Returns the [`PipelineError`](crate::error::PipelineError) a stage reported when a validation
    /// abort or a writer failure tears the phase down.
    #[allow(clippy::too_many_arguments, reason = "phase 2 receives its independently configured stage dependencies")]
    pub(crate) fn run_phase2(
        &self,
        fragment_resolver: FragmentResolver,
        template_resolver: TemplateResolver,
        resolved_count: u64,
        validator: SchemaValidator,
        writer: Box<dyn Writer>,
        controller: Arc<dyn RunController>,
        telemetry: Arc<RunTelemetry>,
    ) -> Result<()> {
        let batch_size = self.config.handoff_size();
        let mode = self.config.mode;
        let series = self.output.series_representation;
        // The extractor, transformer, and validator process N observations (emitted units); the fold
        // and writer emit M entities (one per id). For a current-state run the store has already
        // collapsed each id to one unit, so N == M.
        let emitted_count = fragment_resolver.get_emitted_count().unwrap_or(resolved_count);
        let env = StageEnv {
            channel_policy: self.config.channel_policy,
            reporter: self.context.reporter,
            controller,
            telemetry,
        };

        // The assembler scans the store one chunk at a time and emits units in batches of the same
        // size, so single mode assembles and hands over one unit at a time.
        let assembler_rx = spawn_assembler_thread(Box::new(fragment_resolver), emitted_count, batch_size, &env);
        let extractor_rx = spawn_extractor_thread(assembler_rx, template_resolver, emitted_count, self.config.extraction, mode, env.clone());
        let transformer_rx = spawn_transformer_thread(extractor_rx, emitted_count, mode, env.clone());
        // A series run inserts the aggregator between the validator and the writer; a current-state run
        // wires the validator straight to the writer, so it pays nothing for the fold.
        let validator_downstream = if series { Stage::Aggregator } else { Stage::Writer };
        let validator_rx = spawn_validator_thread(
            transformer_rx,
            validator,
            emitted_count,
            ValidationSettings {
                mode: self.output.validation_mode,
                report_path: self.output.validation_report_path.clone(),
                downstream: validator_downstream,
            },
            env.clone(),
        );
        let writer_input = if series {
            spawn_aggregator_thread(validator_rx, resolved_count, env.clone())
        } else {
            validator_rx
        };
        let writer_rx = spawn_writer_thread(writer_input, writer, resolved_count, env);

        drain_completion(writer_rx)
    }
}

#[cfg(test)]
mod tests {
    use cassiopeia_expander::{compiler::ExpanderCompiler, expander::Expander, generic::GenericExpander, router::MappingRouter};
    use cassiopeia_extractor::{dropped_attributes::DroppedAttributes, entity_extractor::EntityExtractor, extractor::Extractor};
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, record::Record};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::entity::{
        NgsiLdEntity,
        attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper},
        name::NameBuf,
    };
    use cassiopeia_resolver::{
        entity_source::EntitySource,
        entity_store::dashmap_latest_store::DashMapLatestEntityStore,
        fragment_resolver::FragmentResolver,
        fragment_sink::FragmentSink,
        relationship_store::dashmap_store::DashMapRelationshipStore,
    };
    use cassiopeia_transformer::{ngsi_ld_transformer::NgsiLdTransformer, transformer::Transformer};
    use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
    use serde_json::{Map, Value, json};
    use std::{ops::ControlFlow, path::Path, sync::Arc};
    use urn_rs::Urn;

    #[test]
    fn phase1_sink_feeds_phase2_source_across_the_split_traits() {
        // Exercises the resolver seam through the same trait objects the pipeline wires: the resolver
        // sinks fragments as a `FragmentSink`, then the assembler drives it as a boxed `EntitySource`
        // via `drive_assembly`, the way `spawn_assembler_thread` consumes it.
        let mut runner = TemplateRunner::new();
        let document = r#"{ version: "v4", dataModel: "AirQualityObserved", identity: { entityName: "Station-{{ id }}" }, attributes: { temperature: { source: "{{ temperature }}" } } }"#;
        let mapping = Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap());
        let resolver = runner.resolver();
        let fragment_resolver = FragmentResolver::new(Box::new(DashMapLatestEntityStore::new()), Box::new(DashMapRelationshipStore::new()), resolver);

        let sink: &dyn FragmentSink = &fragment_resolver;
        for id in ["urn:ngsi-ld:Station:1", "urn:ngsi-ld:Station:2"] {
            let fragment = Fragment::new(json!({"temperature": 20}), id.parse().unwrap(), None, None);
            sink.resolve(Mapped::new(fragment, Arc::clone(&mapping))).unwrap();
        }

        let source: Box<dyn EntitySource> = Box::new(fragment_resolver);
        assert_eq!(source.get_unique_entity_count().unwrap(), 2);

        let mut ids: Vec<String> = Vec::new();
        source
            .drive_assembly(8, &mut |result| {
                ids.push(result.unwrap().id().to_string());
                ControlFlow::Continue(())
            })
            .unwrap();
        ids.sort();

        assert_eq!(ids, vec!["urn:ngsi-ld:Station:1".to_string(), "urn:ngsi-ld:Station:2".to_string()]);
    }

    /// A `Route` whose `servesAirports` `ListRelationship` declares one `datasetId`-tagged instance per
    /// record field in `fields` (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE 19).
    fn route_mapping(fields: &[&str]) -> String {
        let instances: Vec<String> = fields
            .iter()
            .map(|field| format!(r#"{{ source: "{{{{ {field} }}}}", properties: {{ datasetId: {{ source: "urn:ngsi-ld:dataset:role:{field}" }} }} }}"#))
            .collect();
        format!(
            r#"{{
                version: "v4",
                dataModel: "Route",
                identity: {{ entityName: "R-{{{{ id }}}}" }},
                attributes: {{
                    servesAirports: {{ type: "ListRelationship", target: {{ entity: "Airport" }}, instances: [{}] }},
                }},
            }}"#,
            instances.join(", ")
        )
    }

    /// Runs one record through every stage that shapes an entity's relationships: the expander mints
    /// the objects, the resolver stores and assembles them, the extractor records the per-instance
    /// metadata, and the transformer builds the NGSI-LD attribute.
    fn route_through_the_stages(document: &str, record: Value) -> NgsiLdEntity {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, &mut runner);
        let mapping = Arc::new(mapping);
        let resolver = runner.resolver();
        let Value::Object(data) = record else {
            panic!("test record must be a JSON object");
        };

        let expander = GenericExpander::new(MappingRouter::Single(Arc::clone(&mapping)), resolver.clone(), Map::new());
        let fragments = expander.expand(Record::new(None, data)).unwrap();
        let fragment_resolver = FragmentResolver::new(
            Box::new(DashMapLatestEntityStore::new()),
            Box::new(DashMapRelationshipStore::new()),
            resolver.clone(),
        );
        let id = fragments[0].inner().target_urn().clone();
        for fragment in fragments {
            fragment_resolver.resolve(fragment).unwrap();
        }
        let mut units = fragment_resolver.assemble(&id).unwrap();
        assert_eq!(units.len(), 1);

        let extracted = EntityExtractor::new(resolver).extract(units.remove(0), &DroppedAttributes::new()).unwrap();
        NgsiLdTransformer::new().transform(extracted, &UnreadableTimestamps::new()).unwrap()
    }

    /// Every `ListRelationship` instance of `attribute`, as its object list and its `datasetId`.
    fn list_relationship_instances(entity: &NgsiLdEntity, attribute: &str) -> Vec<(Vec<String>, Option<String>)> {
        let Some(NgsiLdAttributeWrapper::Multi(instances)) = entity.attributes.get(&NameBuf::new(attribute).unwrap()) else {
            panic!("expected a multi-instance attribute");
        };
        instances
            .iter()
            .map(|instance| {
                let NgsiLdAttribute::ListRelationship(list) = instance else {
                    panic!("expected a list relationship instance, got {instance:?}");
                };
                (
                    list.object_list.iter().map(Urn::to_string).collect(),
                    list.dataset_id.as_ref().map(Urn::to_string),
                )
            })
            .collect()
    }

    #[test]
    fn a_relationship_instance_whose_source_names_no_target_is_omitted_and_the_next_keeps_its_dataset_id() {
        let document = r#"{
            version: "v4",
            dataModel: "Flight",
            identity: { entityName: "F-{{ id }}" },
            attributes: {
                servesAirport: {
                    type: "Relationship",
                    target: { entity: "Airport" },
                    instances: [
                        { source: "{{ departure }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:departure" } } },
                        { source: "{{ via }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:via" } } },
                        { source: "{{ arrival }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:arrival" } } },
                    ],
                },
            },
        }"#;

        // `•` is present text, yet it leaves no URN-safe character, so the via instance names no target.
        let entity = route_through_the_stages(document, json!({"id": 1, "departure": "535", "via": "•", "arrival": "340"}));

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = entity.attributes.get(&NameBuf::new("servesAirport").unwrap()) else {
            panic!("expected a multi-instance attribute");
        };
        let objects_and_dataset_ids: Vec<(String, Option<String>)> = instances
            .iter()
            .map(|instance| {
                let NgsiLdAttribute::Relationship(relationship) = instance else {
                    panic!("expected a relationship instance, got {instance:?}");
                };
                (relationship.object.to_string(), relationship.dataset_id.as_ref().map(Urn::to_string))
            })
            .collect();
        assert_eq!(
            objects_and_dataset_ids,
            [
                ("urn:ngsi-ld:Airport:535".to_string(), Some("urn:ngsi-ld:dataset:role:departure".to_string())),
                ("urn:ngsi-ld:Airport:340".to_string(), Some("urn:ngsi-ld:dataset:role:arrival".to_string())),
            ]
        );
    }

    #[test]
    fn an_unmintable_token_leaves_every_list_relationship_instance_with_its_own_objects_and_dataset_id() {
        let entity = route_through_the_stages(
            &route_mapping(&["departure", "arrival"]),
            json!({"id": 1, "departure": "• 535", "arrival": "340 341"}),
        );

        assert_eq!(
            list_relationship_instances(&entity, "servesAirports"),
            [
                (
                    vec!["urn:ngsi-ld:Airport:535".to_string()],
                    Some("urn:ngsi-ld:dataset:role:departure".to_string())
                ),
                (
                    vec!["urn:ngsi-ld:Airport:340".to_string(), "urn:ngsi-ld:Airport:341".to_string()],
                    Some("urn:ngsi-ld:dataset:role:arrival".to_string())
                ),
            ]
        );
    }

    #[test]
    fn a_list_relationship_instance_of_only_unmintable_tokens_is_omitted_and_the_next_keeps_its_dataset_id() {
        let entity = route_through_the_stages(
            &route_mapping(&["departure", "via", "arrival"]),
            json!({"id": 1, "departure": "535", "via": "• ?", "arrival": "340"}),
        );

        assert_eq!(
            list_relationship_instances(&entity, "servesAirports"),
            [
                (
                    vec!["urn:ngsi-ld:Airport:535".to_string()],
                    Some("urn:ngsi-ld:dataset:role:departure".to_string())
                ),
                (
                    vec!["urn:ngsi-ld:Airport:340".to_string()],
                    Some("urn:ngsi-ld:dataset:role:arrival".to_string())
                ),
            ]
        );
    }
}

use crate::{
    context_resolution::resolve_context_source,
    controller::{AtomicBoolController, RunController},
    error::Result,
    input_config::InputConfig,
    memory_sampler::MemorySampler,
    observer::{PipelineProgress, PipelineResult},
    pipeline::Pipeline,
    pipeline_stage::PipelineStage,
    schema_resolution::resolve_custom_schemas,
};
use cassiopeia_common::{signal::MetaSignal, telemetry::run::RunTelemetry};
use cassiopeia_expander::{
    compiler::ExpanderCompiler,
    router::{CollectionRoutes, MappingRouter},
};
use cassiopeia_manifest::mapping_binding::MappingBinding;
use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
use cassiopeia_ngsi_ld::{data_model::DataModelRepository, entity::name::NameBuf};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

impl Pipeline {
    /// Resolves every input into the store, then extracts, transforms, validates, and writes the
    /// resulting entities.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`](crate::error::PipelineError) when a mapping fails to load, the
    /// resolver reports a failure, or a validation abort or writer failure tears the output phase
    /// down.
    pub(crate) fn run_cycle(&self, schedule_meta: &MetaSignal) -> Result<()> {
        let mut runner = TemplateRunner::new();

        // Compile every input's mappings into the shared runner, deduplicating by path so a mapping
        // referenced by two collections (or two inputs) compiles once and is shared behind one `Arc`.
        // Each input's binding becomes a router the expander selects mappings from.
        let mut compiled: HashMap<PathBuf, Arc<Mapping>> = HashMap::new();
        let mut loaded: Vec<(&InputConfig, MappingRouter)> = Vec::with_capacity(self.inputs.len());
        for input in &self.inputs {
            let router = build_router(&input.mapping_binding, &mut compiled, &mut runner)?;
            loaded.push((input, router));
        }

        // Snapshot the resolver only once every mapping's templates are registered, then share it
        // across the expanders, the fragment resolver, and the extractor.
        let template_resolver = runner.resolver();

        let context = resolve_context_source(&loaded, &self.output.global_context_mode, &self.config.schemas_folder, self.context.reporter);

        let fragment_resolver = self.create_resolver(template_resolver.clone())?;
        let writer = self.create_writer(context)?;

        // Each qualified data model tells the validator which repository subdirectory holds its
        // schema, so a catalog schema at `<folder>/<repository>/<Type>.json` is found rather than
        // only a flat file at the folder root.
        let repositories: HashMap<NameBuf, DataModelRepository> = compiled
            .values()
            .filter_map(|mapping| {
                let data_model = mapping.data_model();
                data_model.repository().map(|repository| (data_model.entity_type().clone(), repository.clone()))
            })
            .collect();

        // Custom schemas are resolved once here: a remote source is downloaded to a temp file whose
        // directory `resolved_schemas` owns. That binding is held until after `run_phase2` returns so
        // the temp files outlive the validator's lazy, first-use schema compilation.
        let resolved_schemas = resolve_custom_schemas(
            &loaded,
            self.output.global_validation_schema.as_ref(),
            self.context.reporter,
            &self.config.default_user_agent,
        )?;
        let validator = self.create_validator(repositories, resolved_schemas.by_type);

        // One cancellation source per cycle is shared by all pipeline stages; it reads the
        // process-lifetime shutdown flag set by the signal handler.
        let controller: Arc<dyn RunController> = Arc::new(AtomicBoolController::new(self.context.shutdown));
        let telemetry = Arc::new(RunTelemetry::new());
        // Sample process resident memory for the whole run; dropped just before the final snapshot so
        // the peak and average are complete when the summary reads them.
        let memory_sampler = MemorySampler::start(Arc::clone(&telemetry));

        let resolved_count = self.run_phase1(
            loaded,
            &template_resolver,
            &fragment_resolver,
            schedule_meta,
            &controller,
            Arc::clone(&telemetry),
        )?;
        telemetry.set_unique_entities(resolved_count);
        self.context.observer.on_progress(&PipelineProgress {
            stage: PipelineStage::ResolverSink,
            processed: resolved_count,
            total: None,
            telemetry: Some(telemetry.snapshot()),
        });

        let phase2_result = self.run_phase2(
            fragment_resolver,
            template_resolver,
            resolved_count,
            validator,
            writer,
            controller,
            Arc::clone(&telemetry),
        );
        drop(memory_sampler);
        let telemetry_snapshot = telemetry.snapshot();
        // The reasons a run's failures grouped under are the reporter's own tally; the pipeline
        // hands it none, and the deduplicating middleware appends what it collected.
        self.context.reporter.summary(&telemetry_snapshot, &[]);
        self.context.observer.on_progress(&PipelineProgress {
            stage: PipelineStage::Writer,
            processed: telemetry_snapshot.counters.entities_written,
            total: Some(telemetry_snapshot.counters.unique_entities),
            telemetry: Some(telemetry_snapshot.clone()),
        });
        self.context.observer.on_complete(&PipelineResult {
            input_records: telemetry_snapshot.counters.input_records,
            output_entities: telemetry_snapshot.counters.entities_written,
            // A fatal phase-2 abort emits no per-entity counter, so the completion result reflects it
            // even when the stage counted no failing entity.
            errors: telemetry_snapshot.counters.errors.max(u64::from(phase2_result.is_err())),
            warnings: telemetry_snapshot.counters.warnings,
            telemetry: telemetry_snapshot,
        });

        // The summary above is reported first, then the typed error propagates so the run's exit
        // status records the failure.
        phase2_result
    }
}

/// Builds the mapping router for one input's binding, compiling each distinct path once.
fn build_router(binding: &MappingBinding, compiled: &mut HashMap<PathBuf, Arc<Mapping>>, runner: &mut TemplateRunner) -> Result<MappingRouter> {
    match binding {
        MappingBinding::Single { mapping } => Ok(MappingRouter::Single(compile_mapping(compiled, runner, mapping)?)),
        MappingBinding::Collections { mappings } => {
            let mut routes = CollectionRoutes::default();
            routes.reserve(mappings.len());
            for entry in mappings {
                let mapping = compile_mapping(compiled, runner, entry.mapping())?;
                routes.insert(entry.collection().clone(), mapping);
            }
            Ok(MappingRouter::Collections(routes))
        }
    }
}

/// Loads and compiles a mapping, reusing an already-compiled one for the same path.
fn compile_mapping(compiled: &mut HashMap<PathBuf, Arc<Mapping>>, runner: &mut TemplateRunner, path: &Path) -> Result<Arc<Mapping>> {
    if let Some(existing) = compiled.get(path) {
        return Ok(Arc::clone(existing));
    }

    let mut mapping = Mapping::from_file(path, runner)?;
    ExpanderCompiler::compile(&mut mapping, runner);
    let mapping = Arc::new(mapping);
    compiled.insert(path.to_path_buf(), Arc::clone(&mapping));
    Ok(mapping)
}

#[cfg(test)]
mod tests {
    use crate::cycle::build_router;
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_expander::router::MappingRouter;
    use cassiopeia_manifest::mapping_binding::{CollectionMapping, MappingBinding};
    use cassiopeia_mapping::template::runner::TemplateRunner;
    use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
    use temp_dir::TempDir;

    fn write_mapping(dir: &TempDir, name: &str, data_model: &str) -> PathBuf {
        let path = dir.path().join(name);
        let document = format!(
            r#"{{ version: "v4", dataModel: "{data_model}", identity: {{ entityName: "E-{{{{ id }}}}" }}, attributes: {{ v: {{ source: "{{{{ v }}}}" }} }} }}"#
        );
        fs::write(&path, document).unwrap();
        path
    }

    #[test]
    fn a_single_binding_builds_a_single_router() {
        let dir = TempDir::new().unwrap();
        let path = write_mapping(&dir, "camera.json5", "Camera");
        let binding = MappingBinding::Single { mapping: path };

        let mut compiled = HashMap::new();
        let mut runner = TemplateRunner::new();
        let router = build_router(&binding, &mut compiled, &mut runner).unwrap();

        assert!(matches!(router, MappingRouter::Single(_)));
        assert_eq!(compiled.len(), 1);
    }

    #[test]
    fn collections_pointing_at_one_path_compile_once_and_share_an_arc() {
        let dir = TempDir::new().unwrap();
        let camera = write_mapping(&dir, "camera.json5", "Camera");
        let sensor = write_mapping(&dir, "sensor.json5", "Sensor");
        let binding = MappingBinding::Collections {
            mappings: vec![
                CollectionMapping::new(CollectionName::from("Camera"), camera.clone()),
                CollectionMapping::new(CollectionName::from("Camera Area"), camera),
                CollectionMapping::new(CollectionName::from("Flowcount"), sensor),
            ],
        };

        let mut compiled = HashMap::new();
        let mut runner = TemplateRunner::new();
        let router = build_router(&binding, &mut compiled, &mut runner).unwrap();

        let MappingRouter::Collections(by_label) = router else {
            panic!("a mappings binding must build a collections router");
        };
        // Two distinct paths compiled despite three collections; the shared path is compiled once.
        assert_eq!(compiled.len(), 2);
        assert_eq!(by_label.len(), 3);
        let camera = &by_label[&CollectionName::from("Camera")];
        let camera_area = &by_label[&CollectionName::from("Camera Area")];
        assert!(Arc::ptr_eq(camera, camera_area));
        assert!(!Arc::ptr_eq(camera, &by_label[&CollectionName::from("Flowcount")]));
    }
}

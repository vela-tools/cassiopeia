use crate::{
    error::PipelineError,
    pipeline_config::ExtractionParallelism,
    pipeline_stage::PipelineStage,
    stages::{
        coded_error::CodedError,
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        skipped_records::SkippedRecords,
        stage_env::StageEnv,
        unreadable_timestamp_report::{TimestampLoss, report_unreadable_timestamps},
    },
};
use cassiopeia_common::{
    batch::Batch,
    channel::ChannelReceiver,
    parallelism::Parallelism,
    pipeline_mode::PipelineMode,
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_diagnostic::{code::diagnostic_code::DiagnosticCode, context_field::ContextField, diagnostic_builder::from_error, severity::Severity};
use cassiopeia_extractor::{dropped_geometries::DroppedGeometries, entity_extractor::EntityExtractor, error::ExtractionError, extractor::Extractor};
use cassiopeia_ir::{assembled_entity::AssembledEntity, entity::Entity, mapped::Mapped};
use cassiopeia_mapping::template::resolver::TemplateResolver;
use cassiopeia_reporter::{guard::StageGuard, reporter::Reporter};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use std::{num::NonZeroU64, ops::ControlFlow, sync::Arc};

/// Resolves each assembled entity's attribute values from its source data.
///
/// An entity that fails to extract is counted as a stage warning and skipped, so one extraction
/// failure does not abort the run. An attribute whose value the declared transformation refuses is
/// dropped rather than emitted: a geometry the mapping did not authorise converting, or text that
/// reads as no date-time. The entity still goes downstream, the refusal is named once however many
/// records it affected, and every affected attribute is counted as a warning.
struct ExtractorProcessor {
    /// The extractor that resolves attribute values, configured for the run's parallelism.
    extractor: EntityExtractor,
    /// The reporter each distinct refusal is named on.
    reporter: &'static dyn Reporter,
    /// The run telemetry warnings are counted into, so the run summary's warning total matches what
    /// the stage reported.
    telemetry: Arc<RunTelemetry>,
}

impl ExtractorProcessor {
    /// Names each distinct refusal once and returns how many attributes were dropped in total.
    ///
    /// The reporter collapses a repeated diagnostic into one line plus a count, so the per-batch sink
    /// only has to avoid repeating itself within its own batch. Each refusal carries the attribute it
    /// cost and its own geometry code, so the run summary can say which structural rule the source
    /// data broke.
    fn report_dropped_geometries(&self, dropped: DroppedGeometries) -> u64 {
        if dropped.is_empty() {
            return 0;
        }

        let mut total: u64 = 0;
        for (entry, count) in dropped.into_entries() {
            let occurrences = NonZeroU64::new(count).unwrap_or(NonZeroU64::MIN);
            self.reporter.report(
                &from_error(Severity::Warning, DiagnosticCode::Geometry(entry.refusal.code()), &entry.refusal)
                    .with_context(ContextField::Attribute(entry.attribute))
                    .with_occurrences(occurrences)
                    .build(),
            );
            total = total.saturating_add(count);
        }

        total
    }
}

impl PumpProcessor for ExtractorProcessor {
    type In = AssembledEntity;
    type Out = Mapped<Entity>;

    fn process(&mut self, batch: Batch<AssembledEntity>, stage: &StageGuard, tx: &BatchSender<Mapped<Entity>>) -> ControlFlow<()> {
        // The service span covers the extraction only; the handoff downstream is output wait, and
        // timing them separately keeps the two columns from double-counting the same nanoseconds.
        let service = stage.service_span();
        let dropped = DroppedGeometries::new();
        let unreadable = UnreadableTimestamps::new();
        let mut extracted = Batch::with_capacity(batch.len());
        let mut skipped = SkippedRecords::<_, ExtractionError>::new();
        for result in self.extractor.extract_batch(Vec::from(batch), &dropped, &unreadable) {
            match result {
                Ok(entity) => extracted.push(entity),
                Err(error) => skipped.record(error),
            }
        }
        drop(service);

        let failed = skipped.total();
        if !skipped.is_empty() {
            skipped.report(self.reporter, Severity::Warning);
        }
        let warnings =
            failed + self.report_dropped_geometries(dropped) + report_unreadable_timestamps(self.reporter, TimestampLoss::AttributeValue, unreadable);
        if warnings > 0 {
            stage.warn_inc_by(warnings);
            self.telemetry.add_warnings(warnings);
        }
        send_batch(extracted, tx, stage)
    }
}

/// Spawns the extractor, which resolves each assembled entity's attribute values and forwards it to
/// the transformer. Entities arrive already grouped by the assembler, and each group is extracted
/// across the extractor's parallelism and forwarded as one batch.
pub(crate) fn spawn_extractor_thread(
    receiver: ChannelReceiver<Signal<Batch<AssembledEntity>, PipelineError>>,
    template_resolver: TemplateResolver,
    processed_count: u64,
    extraction: ExtractionParallelism,
    mode: PipelineMode,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<Mapped<Entity>>, PipelineError>> {
    let parallelism = match (mode, extraction) {
        (PipelineMode::Single, _) | (PipelineMode::Batch, ExtractionParallelism::Sequential) => Parallelism::Sequential,
        (PipelineMode::Batch, ExtractionParallelism::Parallel) => Parallelism::Parallel,
    };
    let extractor = EntityExtractor::new(template_resolver).with_parallelism(parallelism);
    let reporter = env.reporter;
    let telemetry = Arc::clone(&env.telemetry);

    run_pump_stage(
        ExtractorProcessor {
            extractor,
            reporter,
            telemetry,
        },
        receiver,
        PumpConfig {
            stage: PipelineStage::Extractor,
            set_length: Some(processed_count),
            expected_stops: 1,
            boundary: ChannelBoundary::between(Stage::Extractor, Stage::Transformer),
            env,
        },
    )
}

/// Hands one batch of successfully extracted entities downstream, counting them, and breaks only
/// when the receiver is gone.
fn send_batch(batch: Batch<Mapped<Entity>>, tx: &BatchSender<Mapped<Entity>>, stage: &StageGuard) -> ControlFlow<()> {
    if batch.is_empty() {
        return ControlFlow::Continue(());
    }
    let count = batch.count();
    if stage.measure_output_wait(|| tx.send(Signal::Data(batch)).is_ok()) {
        stage.inc_by(count);
        ControlFlow::Continue(())
    } else {
        ControlFlow::Break(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::RunController,
        pipeline_config::ExtractionParallelism,
        stages::{extractor::spawn_extractor_thread, stage_env::StageEnv},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        pipeline_mode::PipelineMode,
        signal::Signal,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, extractor_code::ExtractorCode, geometry_code::GeometryCode};
    use cassiopeia_expander::compiler::ExpanderCompiler;
    use cassiopeia_ir::{assembled_entity::AssembledEntity, entity::Entity, relationships::Relationships};
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{resolver::TemplateResolver, runner::TemplateRunner},
    };
    use cassiopeia_reporter::reporter::Reporter;
    use serde_json::{Value as JsonValue, json};
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    /// A controller that never cancels, so the extractor runs to completion.
    struct NeverCancel;

    impl RunController for NeverCancel {
        fn should_cancel(&self) -> bool {
            false
        }
    }

    /// A mapping asking for a `Polygon` `location`, with or without a conversion that authorises the
    /// loss a `MultiPolygon` source would otherwise cause.
    fn mapping(geometry_block: &str) -> (TemplateResolver, Arc<Mapping>) {
        let document = format!(
            r#"{{
                version: "v4",
                dataModel: "Zone",
                identity: {{ entityName: "Zone-{{{{ id }}}}" }},
                attributes: {{
                    location: {{ type: "GeoProperty", transformation: "polygon", {geometry_block} source: "{{{{ geometry }}}}" }},
                }},
            }}"#
        );
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(&document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, &mut runner);

        (runner.resolver(), Arc::new(mapping))
    }

    /// A source record whose geometry is a `MultiPolygon` of two surfaces.
    fn record(id: &str) -> JsonValue {
        json!({
            "id": id,
            "geometry": {
                "type": "MultiPolygon",
                "coordinates": [
                    [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                    [[[5.0, 5.0], [8.0, 5.0], [8.0, 8.0], [5.0, 5.0]]],
                ],
            },
        })
    }

    /// A mapping asking for a `datetime` `dateObserved`.
    fn timestamp_mapping() -> (TemplateResolver, Arc<Mapping>) {
        let document = r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: {
                    dateObserved: { type: "Property", transformation: "datetime", source: "{{ ts }}" },
                },
            }"#;
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, &mut runner);

        (runner.resolver(), Arc::new(mapping))
    }

    /// Runs one batch of two Sensor entities carrying `ts` through the extractor stage.
    fn run_timestamp_batch(ts: &str, reporter: &'static dyn Reporter) -> (usize, u64) {
        let (resolver, mapping) = timestamp_mapping();
        let records = ["001", "002"]
            .into_iter()
            .map(|id| (format!("urn:ngsi-ld:Sensor:{id}"), json!({"id": id, "ts": ts})))
            .collect();

        run_stage(resolver, &mapping, records, reporter)
    }

    /// Runs one batch of two entities through the extractor stage and returns how many entities
    /// reached the output and how many warnings the run counted.
    fn run_batch(geometry_block: &str, reporter: &'static dyn Reporter) -> (usize, u64) {
        let (resolver, mapping) = mapping(geometry_block);
        let records = ["001", "002"].into_iter().map(|id| (format!("urn:ngsi-ld:Zone:{id}"), record(id))).collect();

        run_stage(resolver, &mapping, records, reporter)
    }

    /// Runs one batch of `records` through the extractor stage and returns how many entities reached
    /// the output and how many warnings the run counted.
    fn run_stage(resolver: TemplateResolver, mapping: &Arc<Mapping>, records: Vec<(String, JsonValue)>, reporter: &'static dyn Reporter) -> (usize, u64) {
        let telemetry = Arc::new(RunTelemetry::new());
        let env = StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter,
            controller: Arc::new(NeverCancel),
            telemetry: Arc::clone(&telemetry),
        };

        let entities: Vec<AssembledEntity> = records
            .into_iter()
            .map(|(id, record)| {
                let urn: Urn = id.parse().unwrap();
                let entity = Entity::new(urn, record, None, Relationships::default(), None);
                AssembledEntity::from_single(entity, Arc::clone(mapping))
            })
            .collect();

        let (tx, rx) = channel(ChannelPolicy::Unbounded);
        tx.send(Signal::Data(Batch::from(entities))).unwrap();
        tx.send(Signal::Stop).unwrap();
        drop(tx);

        let output: Vec<_> = spawn_extractor_thread(rx, resolver, 0, ExtractionParallelism::Sequential, PipelineMode::Batch, env)
            .into_iter()
            .collect();

        let forwarded = output
            .iter()
            .filter_map(|signal| match signal {
                Signal::Data(batch) => Some(batch.len()),
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
            })
            .sum();

        (forwarded, telemetry.snapshot().counters.warnings)
    }

    #[test]
    fn a_refused_geometry_counts_one_warning_per_record_and_still_forwards_the_entity() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (forwarded, warnings) = run_batch("", &REPORTER);

        assert_eq!(forwarded, 2);
        assert_eq!(warnings, 2);
    }

    #[test]
    fn a_refused_geometry_is_named_once_with_its_own_code_and_the_attribute_it_cost() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (_, warnings) = run_batch("", &REPORTER);

        let reported = REPORTER.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, warnings);
        assert_eq!(reported[0].code, DiagnosticCode::Geometry(GeometryCode::AmbiguousMultiGeometry));
        assert!(reported[0].headline.contains("MultiPolygon"));
    }

    #[test]
    fn an_unreadable_timestamp_counts_one_warning_per_record_and_still_forwards_the_entity() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (forwarded, warnings) = run_timestamp_batch("the third of March", &REPORTER);

        assert_eq!(forwarded, 2);
        assert_eq!(warnings, 2);
    }

    #[test]
    fn an_unreadable_timestamp_is_named_once_with_its_own_code_and_an_example_spelling() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (_, warnings) = run_timestamp_batch("the third of March", &REPORTER);

        let reported = REPORTER.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, warnings);
        assert_eq!(reported[0].code, DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable));
        assert!(reported[0].headline.contains("dateObserved"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("the third of March"), "{}", reported[0].headline);
    }

    #[test]
    fn a_space_separated_timestamp_with_a_utc_offset_leaves_the_run_without_warnings() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (forwarded, warnings) = run_timestamp_batch("2026-03-01 11:04:35+00:00", &REPORTER);

        assert_eq!(forwarded, 2);
        assert_eq!(warnings, 0);
        assert!(REPORTER.diagnostics().is_empty());
    }

    #[test]
    fn a_declared_conversion_leaves_the_run_without_warnings_or_diagnostics() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (forwarded, warnings) = run_batch(r#"geometry: { convert: "largest" },"#, &REPORTER);

        assert_eq!(forwarded, 2);
        assert_eq!(warnings, 0);
        assert!(REPORTER.diagnostics().is_empty());
    }
}

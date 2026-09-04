use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
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
use cassiopeia_diagnostic::severity::Severity;
use cassiopeia_ir::{entity::Entity, mapped::Mapped};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::{guard::StageGuard, reporter::Reporter};
use cassiopeia_transformer::{error::TransformationError, ngsi_ld_transformer::NgsiLdTransformer, transformer::Transformer};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use std::{ops::ControlFlow, sync::Arc};

/// Builds each resolved entity into an [`NgsiLdEntity`].
///
/// A per-entity build failure is counted as a stage warning and skipped, so one build failure does
/// not abort the run. Failures are grouped by reason and each distinct one is reported once with a
/// count, so a systematic mapping mistake reads as a named problem rather than as a number.
///
/// An attribute whose `observedAt` reads as no instant is emitted without the qualifier rather than
/// dropped, and the loss is named once per attribute and counted as a warning: a value that
/// publishes with nothing anchoring it in time is exactly what nothing downstream can notice.
struct TransformerProcessor {
    /// The transformer that builds NGSI-LD entities, configured for the run's parallelism.
    transformer: NgsiLdTransformer,
    /// The reporter each distinct build failure is named on.
    reporter: &'static dyn Reporter,
    /// The run telemetry warnings are counted into, so a build failure reaches the run summary and
    /// not only the progress bar.
    telemetry: Arc<RunTelemetry>,
}

impl PumpProcessor for TransformerProcessor {
    type In = Mapped<Entity>;
    type Out = NgsiLdEntity;

    fn process(&mut self, batch: Batch<Mapped<Entity>>, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) -> ControlFlow<()> {
        // The service span covers the transform only; the handoff downstream is output wait.
        let service = stage.service_span();
        let unreadable = UnreadableTimestamps::new();
        let mut transformed = Batch::with_capacity(batch.len());
        let mut skipped = SkippedRecords::<_, TransformationError>::new();
        for result in self.transformer.transform_batch(Vec::from(batch), &unreadable) {
            match result {
                Ok(entity) => transformed.push(entity),
                Err(error) => skipped.record(error),
            }
        }
        drop(service);

        let failed = skipped.total();
        if !skipped.is_empty() {
            skipped.report(self.reporter, Severity::Warning);
        }
        let warnings = failed + report_unreadable_timestamps(self.reporter, TimestampLoss::ObservedAt, unreadable);
        if warnings > 0 {
            stage.warn_inc_by(warnings);
            self.telemetry.add_warnings(warnings);
        }
        send_batch(transformed, tx, stage)
    }
}

/// Spawns the transformer, which turns each resolved entity into an [`NgsiLdEntity`]. Entities
/// arrive already grouped, and each group is built across the transformer's parallelism and
/// forwarded as one batch.
pub(crate) fn spawn_transformer_thread(
    receiver: ChannelReceiver<Signal<Batch<Mapped<Entity>>, PipelineError>>,
    processed_count: u64,
    mode: PipelineMode,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>> {
    let parallelism = match mode {
        PipelineMode::Single => Parallelism::Sequential,
        PipelineMode::Batch => Parallelism::Parallel,
    };
    let transformer = NgsiLdTransformer::new().with_parallelism(parallelism);
    let reporter = env.reporter;
    let telemetry = Arc::clone(&env.telemetry);

    run_pump_stage(
        TransformerProcessor {
            transformer,
            reporter,
            telemetry,
        },
        receiver,
        PumpConfig {
            stage: PipelineStage::Transformer,
            set_length: Some(processed_count),
            expected_stops: 1,
            boundary: ChannelBoundary::between(Stage::Transformer, Stage::Validator),
            env,
        },
    )
}

/// Hands one batch of successfully built entities downstream, counting them, and breaks only when
/// the receiver is gone.
fn send_batch(batch: Batch<NgsiLdEntity>, tx: &BatchSender<NgsiLdEntity>, stage: &StageGuard) -> ControlFlow<()> {
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
        stages::{stage_env::StageEnv, transformer::spawn_transformer_thread},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        pipeline_mode::PipelineMode,
        signal::Signal,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, transform_code::TransformCode};
    use cassiopeia_ir::{entity::Entity, mapped::Mapped, metadata::MetadataStorage, sub_attribute::SubAttribute};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::{
        entity::{attribute::NgsiLdAttributeKind, name::NameBuf},
        value::types::{Number, Value},
    };
    use cassiopeia_reporter::reporter::Reporter;
    use indexmap::IndexMap;
    use serde_json::json;
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    /// A controller that never cancels, so the transformer runs to completion.
    struct NeverCancel;

    impl RunController for NeverCancel {
        fn should_cancel(&self) -> bool {
            false
        }
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    /// A mapping whose `temperature` carries an `observedAt` qualifier.
    fn mapping() -> Arc<Mapping> {
        let document = r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "S-{{ id }}" },
                attributes: {
                    temperature: { source: "{{ t }}", transformation: "float", properties: { observedAt: { source: "{{ ts }}" } } },
                },
            }"#;
        let mut runner = TemplateRunner::new();

        Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap())
    }

    /// One extracted entity whose `temperature` carries `observed_at` as its `observedAt` text.
    fn entity(id: &str, observed_at: &str, mapping: &Arc<Mapping>) -> Mapped<Entity> {
        let urn: Urn = format!("urn:ngsi-ld:Sensor:{id}").parse().unwrap();
        let values = IndexMap::from_iter([(name("temperature"), Value::Number(Number::Float(20.0)))]);
        let shared = IndexMap::from_iter([(
            name("observedAt"),
            SubAttribute::new(NgsiLdAttributeKind::Property, json!(observed_at), IndexMap::default()),
        )]);
        let metadata = IndexMap::from_iter([(name("temperature"), MetadataStorage::shared(shared))]);

        let mut entity = Entity::new(urn, json!({}), None, IndexMap::default(), Some(values));
        entity.set_metadata(Some(metadata));

        Mapped::new(entity, Arc::clone(mapping))
    }

    /// Runs one batch of two entities through the transformer stage and returns how many entities
    /// reached the output and how many warnings the run counted.
    fn run_batch(observed_at: &str, reporter: &'static dyn Reporter) -> (usize, u64) {
        let mapping = mapping();
        let telemetry = Arc::new(RunTelemetry::new());
        let env = StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter,
            controller: Arc::new(NeverCancel),
            telemetry: Arc::clone(&telemetry),
        };
        let entities = vec![entity("001", observed_at, &mapping), entity("002", observed_at, &mapping)];

        let (tx, rx) = channel(ChannelPolicy::Unbounded);
        tx.send(Signal::Data(Batch::from(entities))).unwrap();
        tx.send(Signal::Stop).unwrap();
        drop(tx);

        let output: Vec<_> = spawn_transformer_thread(rx, 0, PipelineMode::Batch, env).into_iter().collect();

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
    fn an_unreadable_observed_at_counts_one_warning_per_record_and_still_forwards_the_entity() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (forwarded, warnings) = run_batch("the third of March", &REPORTER);

        assert_eq!(forwarded, 2);
        assert_eq!(warnings, 2);
    }

    #[test]
    fn an_unreadable_observed_at_is_named_once_with_its_own_code_and_an_example_spelling() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (_, warnings) = run_batch("the third of March", &REPORTER);

        let reported = REPORTER.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, warnings);
        assert_eq!(reported[0].code, DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable));
        assert!(reported[0].headline.contains("temperature"), "{}", reported[0].headline);
        assert!(reported[0].headline.contains("the third of March"), "{}", reported[0].headline);
    }

    #[test]
    fn a_space_separated_observed_at_with_a_utc_offset_leaves_the_run_without_warnings() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let (forwarded, warnings) = run_batch("2026-03-01 11:04:35+00:00", &REPORTER);

        assert_eq!(forwarded, 2);
        assert_eq!(warnings, 0);
        assert!(REPORTER.diagnostics().is_empty());
    }
}

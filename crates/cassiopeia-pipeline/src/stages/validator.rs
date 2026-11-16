use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
        enforcement::{EnforcedBatch, Enforcement, ValidationSettings},
        forwarded::Forwarded,
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        stage_env::StageEnv,
        stream_outcome::StreamOutcome,
    },
};
use cassiopeia_common::{batch::Batch, channel::ChannelReceiver, signal::Signal, stage::Stage, telemetry::channel_boundary::ChannelBoundary};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::guard::StageGuard;
use cassiopeia_validator::{carried_members::CarriedMembers, schema_validator::SchemaValidator, schema_verdict::ValidationOutcome, validator::Validator};
use std::{marker::PhantomData, ops::ControlFlow};

/// Checks each entity against its JSON Schema and enforces the run's validation mode on the verdict,
/// forwarding each entity it lets through as `O`, which decides where the members a schema requires
/// at the top level are decided.
struct ValidatorProcessor<O> {
    /// The schema validator that renders the verdict on each entity.
    validator: SchemaValidator,
    /// The run's validation mode, applied to each verdict.
    enforcement: Enforcement,
    /// What each entity let through is forwarded as.
    forwarded: PhantomData<O>,
}

impl<O: Forwarded> PumpProcessor for ValidatorProcessor<O> {
    type In = NgsiLdEntity;
    type Out = O;

    fn process(&mut self, batch: Batch<NgsiLdEntity>, stage: &StageGuard, tx: &BatchSender<O>) -> ControlFlow<()> {
        self.enforcement.observe(batch.count());

        // The service span covers the schema check only; the handoff downstream is output wait.
        let service = stage.service_span();
        let verdicts = self.validator.validate_batch(&batch, self.enforcement.diagnostics_level(), O::REQUIRED_MEMBERS);
        drop(service);

        let mut enforced = EnforcedBatch::with_capacity(batch.len());
        for (entity, mut verdict) in batch.into_iter().zip(verdicts) {
            let carried = verdict.as_mut().map_or_else(|_| CarriedMembers::none(), ValidationOutcome::take_carried);
            if enforced
                .admit(&mut self.enforcement, entity, verdict, |entity| O::forward(entity, carried))
                .is_break()
            {
                break;
            }
        }
        enforced.send(&self.enforcement, stage, tx)
    }

    fn finalize(&mut self, _outcome: StreamOutcome, _stage: &StageGuard, _tx: &BatchSender<O>) {
        self.enforcement.finish();
    }
}

/// Spawns the validator, which checks each entity against its JSON Schema and applies the run's
/// validation mode, forwarding each entity it lets through as `O`. Entities arrive already grouped;
/// each group is checked across a Rayon pool, then walked in order to apply the mode.
pub(crate) fn spawn_validator_thread<O: Forwarded>(
    receiver: ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>>,
    validator: SchemaValidator,
    processed_count: u64,
    settings: ValidationSettings,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<O>, PipelineError>> {
    run_pump_stage(
        ValidatorProcessor {
            validator,
            enforcement: Enforcement::new(settings, &env),
            forwarded: PhantomData,
        },
        receiver,
        PumpConfig {
            stage: PipelineStage::Validator,
            set_length: Some(processed_count),
            expected_stops: 1,
            boundary: ChannelBoundary::between(Stage::Validator, O::DOWNSTREAM),
            env,
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::RunController,
        error::PipelineError,
        stages::{enforcement::ValidationSettings, stage_env::StageEnv, validator::spawn_validator_thread},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        representation::NgsiLdRepresentation,
        signal::Signal,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode};
    use cassiopeia_manifest::output::validation_mode::ValidationMode;
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_reporter::{backend::noop::NoopReporter, reporter::Reporter};
    use cassiopeia_validator::{schema_validator::SchemaValidator, schema_validator_config::SchemaValidatorConfig};
    use pretty_assertions::assert_eq;
    use std::{collections::HashMap, sync::Arc};
    use temp_dir::TempDir;
    use urn_rs::Urn;

    static NOOP: NoopReporter = NoopReporter::new();
    static RECORDER: RecordingReporter = RecordingReporter::new();

    /// A controller that never cancels, so the validator runs to completion.
    struct NeverCancel;

    impl RunController for NeverCancel {
        fn should_cancel(&self) -> bool {
            false
        }
    }

    /// A stub entity of the given type; nothing in these tests inspects its attributes.
    fn entity(entity_type: &str, id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new(entity_type).unwrap())
    }

    /// What one run of the validator stage produced.
    struct StageRun {
        output: Vec<Signal<Batch<NgsiLdEntity>, PipelineError>>,
        errors: u64,
        warnings: u64,
    }

    /// Runs `batches` through the validator stage under `mode`, with no schemas on disk.
    fn run_batches(mode: ValidationMode, reporter: &'static dyn Reporter, batches: Vec<Vec<NgsiLdEntity>>) -> StageRun {
        let telemetry = Arc::new(RunTelemetry::new());
        let env = StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter,
            controller: Arc::new(NeverCancel),
            telemetry: Arc::clone(&telemetry),
        };

        let (tx, rx) = channel(ChannelPolicy::Unbounded);
        for batch in batches {
            tx.send(Signal::Data(Batch::from(batch))).unwrap();
        }
        tx.send(Signal::Stop).unwrap();
        drop(tx);

        // An empty schema folder makes every entity's schema absent, which is the verdict the mode
        // matrix routes differently for each mode.
        let schemas = TempDir::new().unwrap();
        let validator = SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: schemas.path().to_path_buf(),
            repositories: HashMap::new(),
            custom_schemas: HashMap::new(),
            representation: NgsiLdRepresentation::Normalized,
        });
        let output = spawn_validator_thread::<NgsiLdEntity>(rx, validator, 0, ValidationSettings { mode, report: None }, env)
            .into_iter()
            .collect();

        let counters = telemetry.snapshot().counters;
        StageRun {
            output,
            errors: counters.errors,
            warnings: counters.warnings,
        }
    }

    /// Every entity that reached the output, flattened across batches in arrival order.
    fn forwarded(output: &[Signal<Batch<NgsiLdEntity>, PipelineError>]) -> usize {
        output
            .iter()
            .filter_map(|signal| match signal {
                Signal::Data(batch) => Some(batch.len()),
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
            })
            .sum()
    }

    #[test]
    fn a_relaxed_batch_forwards_every_entity_in_one_batch() {
        let run = run_batches(
            ValidationMode::Warn,
            &NOOP,
            vec![vec![
                entity("Sensor", "urn:ngsi-ld:Sensor:1"),
                entity("Sensor", "urn:ngsi-ld:Sensor:2"),
                entity("Sensor", "urn:ngsi-ld:Sensor:3"),
            ]],
        );

        assert_eq!(forwarded(&run.output), 3);
        assert_eq!(run.errors, 0);
        assert_eq!(run.warnings, 0);
    }

    #[test]
    fn a_warned_batch_counts_one_warning_per_entity_and_still_forwards_them() {
        // `fail-when-schema` warns on an absent schema rather than aborting, so the whole batch is
        // warned and forwarded: the case a per-batch tally must not collapse to a single warning.
        let run = run_batches(
            ValidationMode::FailWhenSchema,
            &NOOP,
            vec![vec![entity("Sensor", "urn:ngsi-ld:Sensor:1"), entity("Sensor", "urn:ngsi-ld:Sensor:2")]],
        );

        assert_eq!(forwarded(&run.output), 2);
        assert_eq!(run.warnings, 2);
        assert_eq!(run.errors, 0);
    }

    #[test]
    fn an_aborting_batch_forwards_what_preceded_the_offender_and_emits_one_error() {
        // Strict mode aborts on the first absent schema, so nothing precedes the offender here and
        // exactly one error is counted for the batch.
        let run = run_batches(
            ValidationMode::Fail,
            &NOOP,
            vec![vec![entity("Sensor", "urn:ngsi-ld:Sensor:1"), entity("Sensor", "urn:ngsi-ld:Sensor:2")]],
        );

        assert_eq!(forwarded(&run.output), 0);
        assert_eq!(run.errors, 1);
        assert!(run.output.iter().any(|signal| matches!(signal, Signal::Error(_))));
    }

    #[test]
    fn an_aborting_batch_names_the_failing_entity() {
        let run = run_batches(ValidationMode::Fail, &NOOP, vec![vec![entity("Sensor", "urn:ngsi-ld:Sensor:7")]]);

        let message = run
            .output
            .iter()
            .find_map(|signal| match signal {
                Signal::Error(error) => Some(error.to_string()),
                Signal::Start | Signal::Stop | Signal::Data(_) | Signal::Meta(_) => None,
            })
            .expect("an error signal");
        assert!(message.contains("urn:ngsi-ld:Sensor:7"), "{message}");
    }

    #[test]
    fn a_run_of_many_warned_batches_emits_exactly_one_diagnostic_per_type() {
        // Four batches of 250 entities, all warned for the same reason on the same type: the anti-
        // flood guarantee is that this is one diagnostic standing for a thousand entities.
        static REPORTER: RecordingReporter = RecordingReporter::new();
        let batches: Vec<Vec<NgsiLdEntity>> = (0..4)
            .map(|batch| {
                (0..250)
                    .map(|index| entity("AirQualityObserved", &format!("urn:ngsi-ld:AirQualityObserved:ES-{batch}-{index}")))
                    .collect()
            })
            .collect();

        let run = run_batches(ValidationMode::FailWhenSchema, &REPORTER, batches);

        assert_eq!(run.warnings, 1000);
        let diagnostics = REPORTER.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::Schema(SchemaCode::Absent));
        assert_eq!(diagnostics[0].occurrences, 1000);
    }

    #[test]
    fn a_warned_run_names_the_type_and_an_exemplar_without_changing_the_warning_total() {
        let run = run_batches(
            ValidationMode::FailWhenSchema,
            &RECORDER,
            vec![vec![
                entity("Sensor", "urn:ngsi-ld:Sensor:1"),
                entity("Sensor", "urn:ngsi-ld:Sensor:2"),
                entity("Sensor", "urn:ngsi-ld:Sensor:3"),
            ]],
        );

        assert_eq!(run.warnings, 3);
        let diagnostics = RECORDER.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].occurrences, 3);
        assert!(diagnostics[0].headline.contains("Sensor"));
        assert!(diagnostics[0].headline.contains("urn:ngsi-ld:Sensor:1"));
    }

    #[test]
    fn a_clean_run_reports_no_diagnostic() {
        static REPORTER: RecordingReporter = RecordingReporter::new();

        let run = run_batches(ValidationMode::Warn, &REPORTER, vec![vec![entity("Sensor", "urn:ngsi-ld:Sensor:1")]]);

        assert_eq!(run.warnings, 0);
        assert!(REPORTER.diagnostics().is_empty());
    }
}

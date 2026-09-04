use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
        nonconformant_types::NonconformantTypes,
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        stage_env::StageEnv,
        stream_outcome::StreamOutcome,
        validation_abort::abort_error,
        validation_decision::{Decision, Reporting, decide, diagnostics_level},
        validation_report::ReportCollector,
    },
};
use cassiopeia_common::{
    batch::Batch,
    channel::ChannelReceiver,
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_manifest::output::validation_mode::ValidationMode;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::{guard::StageGuard, reporter::Reporter};
use cassiopeia_validator::{
    schema_validator::SchemaValidator,
    schema_verdict::{DiagnosticsLevel, ValidationDiagnostics},
    validator::Validator,
};
use std::{mem, ops::ControlFlow, path::PathBuf, sync::Arc};

/// How the validator stage behaves: how strictly a verdict is enforced, where its report goes, and the
/// stage it forwards to (the fold for a series run, the writer otherwise).
pub(crate) struct ValidationSettings {
    /// How strictly a verdict is enforced before an entity is written.
    pub(crate) mode: ValidationMode,
    /// Where a JSON validation report is written when any entity fails, if requested.
    pub(crate) report_path: Option<PathBuf>,
    /// The stage the validator's output queue drains to, naming the channel boundary.
    pub(crate) downstream: Stage,
}

/// One batch's verdicts, tallied as the batch is walked.
///
/// The three counts are distinct on purpose: a passing entity advances the progress bar, a warned
/// one is completed without advancing it (its warning already showed there), and both are forwarded.
#[derive(Default)]
struct BatchTally {
    /// Entities that conformed and advance the stage's progress.
    passed: u64,
    /// Entities forwarded despite a warning, counted as completed but not as processed.
    warned: u64,
}

/// Checks each entity against its JSON Schema and applies the run's [`ValidationMode`]: in the
/// relaxed mode a failure only warns and the entity is still written; in the fail-fast modes a
/// schema-backed failure (and, under the strict mode, a missing schema) emits an error signal that
/// tears the pipeline down. Failures are optionally recorded in the report written once the stream
/// ends.
///
/// Warnings are grouped by (reason, entity type) across the whole run and reported once at the end,
/// so a stream in which every entity is nonconformant costs one line per type rather than one per
/// entity.
struct ValidatorProcessor {
    /// The schema validator that renders the verdict on each entity.
    validator: SchemaValidator,
    /// How strictly a verdict is enforced.
    validation_mode: ValidationMode,
    /// The failure accumulator, present only when a report path was requested.
    collector: Option<ReportCollector>,
    /// How much work a nonconformant result needs for this run's policy and output.
    diagnostics_level: DiagnosticsLevel,
    /// The entity types warned about, grouped by why, flushed when the stream ends.
    warned_types: NonconformantTypes,
    /// The reporter warnings and the final report-write outcome are logged to.
    reporter: &'static dyn Reporter,
    /// The run telemetry an aborting or warning verdict is counted into, so the run summary's error
    /// and warning totals match the exit status.
    telemetry: Arc<RunTelemetry>,
}

impl PumpProcessor for ValidatorProcessor {
    type In = NgsiLdEntity;
    type Out = NgsiLdEntity;

    fn process(&mut self, batch: Batch<NgsiLdEntity>, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) -> ControlFlow<()> {
        if let Some(collector) = &mut self.collector {
            collector.observe(batch.count());
        }

        // The service span covers the schema check only; the handoff downstream is output wait.
        let service = stage.service_span();
        let verdicts = self.validator.validate_batch(&batch, self.diagnostics_level);
        drop(service);

        let mut forwarded = Batch::with_capacity(batch.len());
        let mut tally = BatchTally::default();
        let mut abort = None;

        for (entity, verdict) in batch.into_iter().zip(verdicts) {
            match decide(self.validation_mode, &verdict) {
                Decision::Forward => {
                    tally.passed += 1;
                    forwarded.push(entity);
                }
                Decision::Warn(reason) => {
                    tally.warned += 1;
                    self.warned_types.record(reason, &entity);
                    if let (Some(collector), Ok(outcome)) = (self.collector.as_mut(), verdict)
                        && let ValidationDiagnostics::Report { entry, .. } = outcome.into_diagnostics()
                    {
                        collector.record(entry);
                    }
                    forwarded.push(entity);
                }
                Decision::Abort => {
                    // Entities decided before this one are still forwarded below; the rest of the
                    // batch is dropped. Fail-fast is at batch granularity.
                    abort = Some(abort_error(verdict, &entity, self.collector.as_mut()));
                    break;
                }
            }
        }

        let sent = self.report_and_send(forwarded, &tally, stage, tx);

        match abort {
            Some(error) => {
                stage.fail(1);
                self.telemetry.add_errors(1);
                let _ = tx.send(Signal::Error(error));
                ControlFlow::Break(())
            }
            None => sent,
        }
    }

    fn finalize(&mut self, _outcome: StreamOutcome, _stage: &StageGuard, _tx: &BatchSender<NgsiLdEntity>) {
        let warned = mem::replace(&mut self.warned_types, NonconformantTypes::new());
        if !warned.is_empty() {
            warned.report(self.reporter);
        }
        if let Some(collector) = &mut self.collector {
            collector.write(self.reporter);
        }
    }
}

impl ValidatorProcessor {
    /// Records one batch's tally and hands the entities that survived it downstream.
    fn report_and_send(&self, batch: Batch<NgsiLdEntity>, tally: &BatchTally, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) -> ControlFlow<()> {
        if tally.warned > 0 {
            stage.warn_inc_by(tally.warned);
            self.telemetry.add_warnings(tally.warned);
        }
        if batch.is_empty() {
            return ControlFlow::Continue(());
        }

        if stage.measure_output_wait(|| tx.send(Signal::Data(batch)).is_ok()) {
            stage.inc_by(tally.passed);
            // A warned entity is still forwarded, so it completes the stage's work, but it is not
            // counted as processed on the bar (`warn_inc_by` already reflected it there).
            stage.complete(tally.warned);
            ControlFlow::Continue(())
        } else {
            stage.fail(tally.passed.saturating_add(tally.warned));
            ControlFlow::Break(())
        }
    }
}

/// Spawns the validator, which checks each entity against its JSON Schema and applies the run's
/// validation mode. Entities arrive already grouped; each group is checked across a Rayon pool, then
/// walked in order to apply the mode.
pub(crate) fn spawn_validator_thread(
    receiver: ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>>,
    validator: SchemaValidator,
    processed_count: u64,
    settings: ValidationSettings,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>> {
    let ValidationSettings {
        mode: validation_mode,
        report_path,
        downstream,
    } = settings;
    let reporting = if report_path.is_some() { Reporting::Enabled } else { Reporting::Disabled };
    let diagnostics_level = diagnostics_level(validation_mode, reporting);
    let collector = report_path.map(ReportCollector::new);
    let reporter = env.reporter;
    let telemetry = Arc::clone(&env.telemetry);

    run_pump_stage(
        ValidatorProcessor {
            validator,
            validation_mode,
            collector,
            diagnostics_level,
            warned_types: NonconformantTypes::new(),
            reporter,
            telemetry,
        },
        receiver,
        PumpConfig {
            stage: PipelineStage::Validator,
            set_length: Some(processed_count),
            expected_stops: 1,
            boundary: ChannelBoundary::between(Stage::Validator, downstream),
            env,
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::RunController,
        error::PipelineError,
        stages::{
            stage_env::StageEnv,
            validator::{ValidationSettings, spawn_validator_thread},
        },
        test_reporter::RecordingReporter,
    };
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        representation::NgsiLdRepresentation,
        signal::Signal,
        skip_null::NgsiLdSkipNull,
        stage::Stage,
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode};
    use cassiopeia_manifest::output::validation_mode::ValidationMode;
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_reporter::{backend::noop::NoopReporter, reporter::Reporter};
    use cassiopeia_validator::{schema_validator::SchemaValidator, schema_validator_config::SchemaValidatorConfig};
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
            skip_null: NgsiLdSkipNull::Skip,
        });
        let output = spawn_validator_thread(
            rx,
            validator,
            0,
            ValidationSettings {
                mode,
                report_path: None,
                downstream: Stage::Writer,
            },
            env,
        )
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

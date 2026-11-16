use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
        enforcement::{EnforcedBatch, Enforcement, ValidationSettings},
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        record_entity::RecordEntity,
        stage_env::StageEnv,
        stream_outcome::StreamOutcome,
    },
};
use cassiopeia_aggregator::{
    aggregator::{Aggregator, Offered},
    temporal_aggregator::TemporalAggregator,
};
use cassiopeia_common::{batch::Batch, channel::ChannelReceiver, signal::Signal, stage::Stage, telemetry::channel_boundary::ChannelBoundary};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::guard::StageGuard;
use cassiopeia_validator::carried_members::CarriedMembers;
use std::{convert, mem, ops::ControlFlow};

/// Drives an [`Aggregator`] over the record entities of a series run, folding each id's into the
/// `EntityTemporal` the run writes, and decides on each folded entity the members its schema requires
/// at the top level, which no record decided.
///
/// The fold carries across batch boundaries: the aggregator holds one id open until a different id
/// arrives, whether or not a batch ends in between, so an id split across two batches still folds
/// into one entity, and the members its records lack are gathered across them too.
struct AggregatorProcessor {
    /// The fold, holding one id open at a time.
    aggregator: TemporalAggregator,
    /// The members every record of the open id lacks so far, which the entity lacks unless a later
    /// record holds them.
    missing: CarriedMembers,
    /// The run's validation mode, applied to the members each folded entity lacks.
    enforcement: Enforcement,
}

impl AggregatorProcessor {
    /// Folds one record entity in, handing back the entity it finished, with the members none of
    /// that entity's records holds, when it begins a new id.
    fn offer(&mut self, record: RecordEntity) -> Option<(NgsiLdEntity, CarriedMembers)> {
        let RecordEntity { entity, carried } = record;
        match self.aggregator.offer(entity) {
            Offered::Opened => {
                self.missing = carried;
                None
            }
            Offered::Joined => {
                self.missing.retain_shared(&carried);
                None
            }
            Offered::Finished(finished) => Some((*finished, mem::replace(&mut self.missing, carried))),
        }
    }

    /// Decides on a folded entity the members none of its records holds, and forwards it unless that
    /// aborts the run.
    fn admit(&mut self, enforced: &mut EnforcedBatch<NgsiLdEntity>, entity: NgsiLdEntity, missing: CarriedMembers) -> ControlFlow<()> {
        let verdict = Ok(missing.into_outcome(&entity, self.enforcement.diagnostics_level()));
        enforced.admit(&mut self.enforcement, entity, verdict, convert::identity)
    }
}

impl PumpProcessor for AggregatorProcessor {
    type In = RecordEntity;
    type Out = NgsiLdEntity;

    fn process(&mut self, batch: Batch<RecordEntity>, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) -> ControlFlow<()> {
        let service = stage.service_span();
        // A batch of N records yields at most N folded entities, and usually far fewer: one per
        // distinct id the batch spans.
        let mut enforced = EnforcedBatch::with_capacity(batch.len());
        for record in batch {
            if let Some((entity, missing)) = self.offer(record)
                && self.admit(&mut enforced, entity, missing).is_break()
            {
                break;
            }
        }
        drop(service);

        enforced.send(&self.enforcement, stage, tx)
    }

    fn finalize(&mut self, outcome: StreamOutcome, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) {
        match outcome {
            // The last id's aggregate is still open when the stream ends; flush it so nothing is
            // dropped.
            StreamOutcome::Clean => {
                let service = stage.service_span();
                let last = self.aggregator.finish();
                drop(service);
                if let Some(entity) = last {
                    let missing = mem::take(&mut self.missing);
                    let mut enforced = EnforcedBatch::with_capacity(1);
                    let _ = self.admit(&mut enforced, entity, missing);
                    let _ = enforced.send(&self.enforcement, stage, tx);
                }
            }
            // A failed or cancelled stream may have stopped before the open entity's last records,
            // so it is not written: it may be incomplete, and a member a missing record holds cannot
            // be told missing.
            StreamOutcome::Failed => {}
        }
        self.enforcement.finish();
    }
}

/// Spawns the aggregator stage. It groups each id's record entities, emitting one folded
/// `EntityTemporal` per id, and enforces the run's validation mode on the members each one's schema
/// requires at the top level. Inserted only for a series-representation run; a current-state run
/// wires the validator straight to the writer.
pub(crate) fn spawn_aggregator_thread(
    receiver: ChannelReceiver<Signal<Batch<RecordEntity>, PipelineError>>,
    entity_count: u64,
    settings: ValidationSettings,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>> {
    run_pump_stage(
        AggregatorProcessor {
            aggregator: TemporalAggregator::new(),
            missing: CarriedMembers::none(),
            enforcement: Enforcement::new(settings, &env),
        },
        receiver,
        PumpConfig {
            stage: PipelineStage::Aggregator,
            set_length: Some(entity_count),
            expected_stops: 1,
            boundary: ChannelBoundary::between(Stage::Aggregator, Stage::Writer),
            env,
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::RunController,
        error::PipelineError,
        pipeline_stage::PipelineStage,
        stages::{aggregator::spawn_aggregator_thread, enforcement::ValidationSettings, record_entity::RecordEntity, stage_env::StageEnv},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        representation::NgsiLdRepresentation,
        signal::{MetaSignal, Signal},
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode};
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_manifest::output::validation_mode::ValidationMode;
    use cassiopeia_ngsi_ld::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, geo_property::NgsiLdGeoProperty, property::NgsiLdProperty},
            name::NameBuf,
        },
        value::types::Value,
    };
    use cassiopeia_reporter::{backend::noop::NoopReporter, reporter::Reporter};
    use cassiopeia_validator::{
        carried_members::CarriedMembers,
        required_members::RequiredMembers,
        schema_validator::SchemaValidator,
        schema_validator_config::SchemaValidatorConfig,
        schema_verdict::DiagnosticsLevel,
        validator::Validator,
    };
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{collections::HashMap, fs, num::NonZeroUsize, sync::Arc};
    use temp_dir::TempDir;
    use urn_rs::Urn;

    static NOOP: NoopReporter = NoopReporter::new();

    /// A controller that never cancels, so the aggregator runs to completion.
    struct NeverCancel;

    impl RunController for NeverCancel {
        fn should_cancel(&self) -> bool {
            false
        }
    }

    fn env(reporter: &'static dyn Reporter, telemetry: &Arc<RunTelemetry>) -> StageEnv {
        StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter,
            controller: Arc::new(NeverCancel),
            telemetry: Arc::clone(telemetry),
        }
    }

    /// A temporal `maxSustainedWind` observation for `id`, stamped at `observed_at`.
    fn wind_entity(id: &str, value: f64, observed_at: &str) -> NgsiLdEntity {
        let mut property = NgsiLdProperty::new(Value::try_from(json!(value)).unwrap());
        property.observed_at = Some(observed_at.parse::<DateTime<Utc>>().unwrap());
        let mut entity = NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("TropicalCyclone").unwrap());
        entity.attributes.insert(
            NameBuf::new("maxSustainedWind").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(property)),
        );
        entity
    }

    /// A temporal `location` observation for `id`, stamped at `observed_at`.
    fn location_entity(id: &str, coordinates: [f64; 2], observed_at: &str) -> NgsiLdEntity {
        let mut geo = NgsiLdGeoProperty::new(NgsiLdGeometry::Point {
            coordinates: coordinates.into(),
        });
        geo.observed_at = Some(observed_at.parse::<DateTime<Utc>>().unwrap());
        let mut entity = NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("TropicalCyclone").unwrap());
        entity.attributes.insert(
            NameBuf::new("location").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::GeoProperty(geo)),
        );
        entity
    }

    /// The record entity of a `maxSustainedWind` observation, lacking no member a schema requires.
    fn wind_observation(id: &str, value: f64, observed_at: &str) -> RecordEntity {
        RecordEntity {
            entity: wind_entity(id, value, observed_at),
            carried: CarriedMembers::none(),
        }
    }

    /// Lets `entities` through a validator whose `TropicalCyclone` schema requires `maxSustainedWind`
    /// and `location`, as a series run does, each carrying the members it lacks.
    fn checked(entities: Vec<NgsiLdEntity>) -> Vec<RecordEntity> {
        let schemas = TempDir::new().unwrap();
        fs::write(
            schemas.path().join("TropicalCyclone.json"),
            r#"{"type": "object", "properties": {"maxSustainedWind": {"type": "number"}}, "required": ["id", "type", "maxSustainedWind", "location"]}"#,
        )
        .unwrap();
        let validator = SchemaValidator::new(SchemaValidatorConfig {
            schemas_folder: schemas.path().to_path_buf(),
            repositories: HashMap::new(),
            custom_schemas: HashMap::new(),
            representation: NgsiLdRepresentation::Simplified,
        });
        let outcomes = validator.validate_batch(&entities, DiagnosticsLevel::None, RequiredMembers::Carried);
        entities
            .into_iter()
            .zip(outcomes)
            .map(|(entity, outcome)| RecordEntity {
                entity,
                carried: outcome.unwrap().take_carried(),
            })
            .collect()
    }

    /// What one run of the aggregator stage produced.
    struct StageRun {
        output: Vec<Signal<Batch<NgsiLdEntity>, PipelineError>>,
        errors: u64,
        warnings: u64,
    }

    /// Feeds a prepared list of signals through the aggregator stage under `mode`, reporting to
    /// `reporter`, and returns the drained output with the errors and warnings it counted.
    fn run_checked(mode: ValidationMode, reporter: &'static dyn Reporter, inputs: Vec<Signal<Batch<RecordEntity>, PipelineError>>) -> StageRun {
        let telemetry = Arc::new(RunTelemetry::new());
        let (tx, rx) = channel(ChannelPolicy::Bounded(NonZeroUsize::new(1024).unwrap()));
        for signal in inputs {
            tx.send(signal).unwrap();
        }
        drop(tx);
        let output = spawn_aggregator_thread(rx, 0, ValidationSettings { mode, report: None }, env(reporter, &telemetry))
            .into_iter()
            .collect();
        let counters = telemetry.snapshot().counters;
        StageRun {
            output,
            errors: counters.errors,
            warnings: counters.warnings,
        }
    }

    /// Feeds a prepared list of signals through the aggregator stage and returns the drained output.
    fn run_stage(inputs: Vec<Signal<Batch<RecordEntity>, PipelineError>>) -> Vec<Signal<Batch<NgsiLdEntity>, PipelineError>> {
        run_checked(ValidationMode::FailWhenSchema, &NOOP, inputs).output
    }

    /// The message of the error a run emitted.
    fn error_message(output: &[Signal<Batch<NgsiLdEntity>, PipelineError>]) -> Vec<String> {
        output
            .iter()
            .filter_map(|signal| match signal {
                Signal::Error(error) => Some(error.to_string()),
                Signal::Start | Signal::Stop | Signal::Data(_) | Signal::Meta(_) => None,
            })
            .collect()
    }

    /// Every folded entity that reached the output, in arrival order.
    fn folded(output: &[Signal<Batch<NgsiLdEntity>, PipelineError>]) -> Vec<&NgsiLdEntity> {
        output
            .iter()
            .filter_map(|signal| match signal {
                Signal::Data(batch) => Some(batch.iter()),
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn the_stage_folds_one_id_and_brackets_its_output_with_start_and_stop() {
        let output = run_stage(vec![
            Signal::Data(Batch::from(vec![
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 30.0, "2005-08-25T12:00:00Z"),
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T18:00:00Z"),
            ])),
            Signal::Stop,
        ]);

        assert!(matches!(output.first(), Some(Signal::Start)));
        assert!(matches!(output.last(), Some(Signal::Stop)));
        let folded = folded(&output);
        assert_eq!(folded.len(), 1);
        assert!(matches!(folded[0].attributes.get("maxSustainedWind"), Some(NgsiLdAttributeWrapper::Multi(_))));
    }

    #[test]
    fn one_ids_run_split_across_two_batches_still_folds_into_a_single_entity() {
        // The fold is stateful across batches: an id whose observations straddle a batch boundary
        // must not emit twice, which is exactly what per-batch state would do.
        let output = run_stage(vec![
            Signal::Data(Batch::from(vec![
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 30.0, "2005-08-25T12:00:00Z"),
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T18:00:00Z"),
            ])),
            Signal::Data(Batch::from(vec![
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 60.0, "2005-08-26T00:00:00Z"),
                wind_observation("urn:ngsi-ld:TropicalCyclone:B", 10.0, "2005-08-26T06:00:00Z"),
            ])),
            Signal::Stop,
        ]);

        let folded = folded(&output);
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].id.to_string(), "urn:ngsi-ld:TropicalCyclone:A");
        assert_eq!(folded[1].id.to_string(), "urn:ngsi-ld:TropicalCyclone:B");
        let NgsiLdAttributeWrapper::Multi(instances) = folded[0].attributes.get("maxSustainedWind").expect("A's folded attribute") else {
            panic!("expected A's three observations folded into a Multi");
        };
        assert_eq!(instances.len(), 3);
    }

    #[test]
    fn several_ids_inside_one_batch_each_emit_their_own_fold() {
        let output = run_stage(vec![
            Signal::Data(Batch::from(vec![
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 1.0, "2005-08-25T12:00:00Z"),
                wind_observation("urn:ngsi-ld:TropicalCyclone:B", 2.0, "2005-08-25T18:00:00Z"),
                wind_observation("urn:ngsi-ld:TropicalCyclone:C", 3.0, "2005-08-26T00:00:00Z"),
            ])),
            Signal::Stop,
        ]);

        let ids: Vec<String> = folded(&output).iter().map(|entity| entity.id.to_string()).collect();
        assert_eq!(
            ids,
            vec![
                "urn:ngsi-ld:TropicalCyclone:A".to_string(),
                "urn:ngsi-ld:TropicalCyclone:B".to_string(),
                "urn:ngsi-ld:TropicalCyclone:C".to_string(),
            ]
        );
    }

    #[test]
    fn error_and_meta_signals_pass_straight_through() {
        let output = run_stage(vec![
            Signal::Meta(MetaSignal::InputCount(3)),
            Signal::Data(Batch::from(vec![wind_observation(
                "urn:ngsi-ld:TropicalCyclone:A",
                1.0,
                "2005-08-25T12:00:00Z",
            )])),
            Signal::Error(PipelineError::StagePanic {
                stage: PipelineStage::Validator,
            }),
            Signal::Stop,
        ]);

        assert!(output.iter().any(|signal| matches!(signal, Signal::Meta(_))));
        assert!(output.iter().any(|signal| matches!(signal, Signal::Error(_))));
    }

    #[test]
    fn the_entity_still_open_when_the_stream_fails_is_not_written() {
        let output = run_stage(vec![
            Signal::Data(Batch::from(vec![
                wind_observation("urn:ngsi-ld:TropicalCyclone:A", 1.0, "2005-08-25T12:00:00Z"),
                wind_observation("urn:ngsi-ld:TropicalCyclone:B", 2.0, "2005-08-25T18:00:00Z"),
            ])),
            Signal::Error(PipelineError::StagePanic {
                stage: PipelineStage::Validator,
            }),
            Signal::Stop,
        ]);

        // A was finished by B's first record before the stream failed; B may lack records.
        let ids: Vec<String> = folded(&output).iter().map(|entity| entity.id.to_string()).collect();
        assert_eq!(ids, ["urn:ngsi-ld:TropicalCyclone:A"]);
    }

    #[test]
    fn records_that_each_lack_a_different_required_member_fold_into_an_entity_that_conforms() {
        static REPORTER: RecordingReporter = RecordingReporter::new();
        let records = checked(vec![
            wind_entity("urn:ngsi-ld:TropicalCyclone:A", 30.0, "2005-08-25T12:00:00Z"),
            location_entity("urn:ngsi-ld:TropicalCyclone:A", [-75.1, 23.1], "2005-08-25T12:00:00Z"),
            wind_entity("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T18:00:00Z"),
        ]);

        let run = run_checked(
            ValidationMode::FailWhenSchema,
            &REPORTER,
            vec![Signal::Data(Batch::from(records)), Signal::Stop],
        );

        assert_eq!(folded(&run.output).len(), 1);
        assert_eq!(error_message(&run.output), Vec::<String>::new());
        assert_eq!((run.errors, run.warnings), (0, 0));
        assert!(REPORTER.diagnostics().is_empty());
    }

    #[test]
    fn an_entity_none_of_whose_records_holds_a_required_member_aborts_the_run_once_naming_it() {
        for mode in [ValidationMode::FailWhenSchema, ValidationMode::Fail] {
            let records = checked(vec![
                wind_entity("urn:ngsi-ld:TropicalCyclone:A", 30.0, "2005-08-25T12:00:00Z"),
                wind_entity("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T18:00:00Z"),
            ]);

            let run = run_checked(mode, &NOOP, vec![Signal::Data(Batch::from(records)), Signal::Stop]);

            assert!(folded(&run.output).is_empty(), "{mode:?}");
            assert_eq!(
                error_message(&run.output),
                [
                    r#"Entity urn:ngsi-ld:TropicalCyclone:A of type 'TropicalCyclone' does not conform to its schema: 1 violation(s), first at '': "location" is a required property"#
                ],
                "{mode:?}"
            );
            assert_eq!((run.errors, run.warnings), (1, 0), "{mode:?}");
        }
    }

    #[test]
    fn an_entity_lacking_a_required_member_is_written_and_counted_once_when_relaxed() {
        static REPORTER: RecordingReporter = RecordingReporter::new();
        let mut records = checked(vec![
            wind_entity("urn:ngsi-ld:TropicalCyclone:A", 30.0, "2005-08-25T12:00:00Z"),
            wind_entity("urn:ngsi-ld:TropicalCyclone:A", 45.0, "2005-08-25T18:00:00Z"),
            wind_entity("urn:ngsi-ld:TropicalCyclone:A", 60.0, "2005-08-26T00:00:00Z"),
        ]);
        let last = records.split_off(2);

        let run = run_checked(
            ValidationMode::Warn,
            &REPORTER,
            vec![Signal::Data(Batch::from(records)), Signal::Data(Batch::from(last)), Signal::Stop],
        );

        assert_eq!(folded(&run.output).len(), 1);
        assert_eq!((run.errors, run.warnings), (0, 1));
        let diagnostics = REPORTER.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::Schema(SchemaCode::Nonconformant));
        assert_eq!(diagnostics[0].occurrences, 1);
        assert_eq!(
            diagnostics[0].headline,
            "1 'TropicalCyclone' entities do not conform to their schema (first: urn:ngsi-ld:TropicalCyclone:A)"
        );
    }

    #[test]
    fn each_entity_is_decided_on_its_own_records() {
        let records = checked(vec![
            wind_entity("urn:ngsi-ld:TropicalCyclone:A", 30.0, "2005-08-25T12:00:00Z"),
            location_entity("urn:ngsi-ld:TropicalCyclone:A", [-75.1, 23.1], "2005-08-25T12:00:00Z"),
            wind_entity("urn:ngsi-ld:TropicalCyclone:B", 10.0, "2005-08-26T06:00:00Z"),
        ]);

        let run = run_checked(ValidationMode::FailWhenSchema, &NOOP, vec![Signal::Data(Batch::from(records)), Signal::Stop]);

        let ids: Vec<String> = folded(&run.output).iter().map(|entity| entity.id.to_string()).collect();
        assert_eq!(ids, ["urn:ngsi-ld:TropicalCyclone:A"]);
        assert_eq!(error_message(&run.output).len(), 1);
        assert!(error_message(&run.output)[0].contains("urn:ngsi-ld:TropicalCyclone:B"));
    }
}

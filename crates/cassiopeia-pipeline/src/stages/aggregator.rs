use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        stage_env::StageEnv,
        stream_outcome::StreamOutcome,
    },
};
use cassiopeia_aggregator::{aggregator::Aggregator, temporal_aggregator::TemporalAggregator};
use cassiopeia_common::{batch::Batch, channel::ChannelReceiver, signal::Signal, stage::Stage, telemetry::channel_boundary::ChannelBoundary};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::guard::StageGuard;
use std::ops::ControlFlow;

/// Drives an [`Aggregator`] over the entity stream, forwarding each folded `EntityTemporal` downstream.
///
/// The processor is thin: the grouping and folding live in the aggregator, and this only plumbs its
/// `offer`/`finish` results onto the channel and counts them on the stage bar. The fold carries
/// across batch boundaries: the aggregator holds one id open until a different id arrives, whether
/// or not a batch ends in between, so an id split across two batches still folds into one entity.
struct AggregatorProcessor {
    aggregator: TemporalAggregator,
}

impl AggregatorProcessor {
    /// Forwards one batch of folded entities, counting them on the stage bar.
    fn emit(batch: Batch<NgsiLdEntity>, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) -> ControlFlow<()> {
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
}

impl PumpProcessor for AggregatorProcessor {
    type In = NgsiLdEntity;
    type Out = NgsiLdEntity;

    fn process(&mut self, batch: Batch<NgsiLdEntity>, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) -> ControlFlow<()> {
        let service = stage.service_span();
        // A batch of N observations yields at most N folded entities, and usually far fewer: one per
        // distinct id the batch spans.
        let mut folded = Batch::with_capacity(batch.len());
        for entity in batch {
            if let Some(complete) = self.aggregator.offer(entity) {
                folded.push(complete);
            }
        }
        drop(service);

        Self::emit(folded, stage, tx)
    }

    fn finalize(&mut self, _outcome: StreamOutcome, stage: &StageGuard, tx: &BatchSender<NgsiLdEntity>) {
        // The last id's aggregate is still open when the stream ends; flush it so nothing is dropped.
        let service = stage.service_span();
        let last = self.aggregator.finish();
        drop(service);

        if let Some(folded) = last {
            let _ = Self::emit(Batch::from(vec![folded]), stage, tx);
        }
    }
}

/// Spawns the aggregator stage. It groups each id's single-instance observations, emitting one folded
/// `EntityTemporal` per id. Inserted only for a series-representation run; a current-state run wires
/// the validator straight to the writer.
pub(crate) fn spawn_aggregator_thread(
    receiver: ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>>,
    entity_count: u64,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<NgsiLdEntity>, PipelineError>> {
    run_pump_stage(
        AggregatorProcessor {
            aggregator: TemporalAggregator::new(),
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
        stages::{aggregator::spawn_aggregator_thread, stage_env::StageEnv},
    };
    use cassiopeia_common::{
        batch::Batch,
        channel::{ChannelPolicy, channel},
        signal::{MetaSignal, Signal},
        telemetry::run::RunTelemetry,
    };
    use cassiopeia_ngsi_ld::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, property::NgsiLdProperty},
            name::NameBuf,
        },
        value::types::Value,
    };
    use cassiopeia_reporter::{backend::noop::NoopReporter, reporter::Reporter};
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::{num::NonZeroUsize, sync::Arc};
    use urn_rs::Urn;

    static NOOP: NoopReporter = NoopReporter::new();

    /// A controller that never cancels, so the aggregator runs to completion.
    struct NeverCancel;

    impl RunController for NeverCancel {
        fn should_cancel(&self) -> bool {
            false
        }
    }

    fn env() -> StageEnv {
        StageEnv {
            channel_policy: ChannelPolicy::Unbounded,
            reporter: &NOOP as &'static dyn Reporter,
            controller: Arc::new(NeverCancel),
            telemetry: Arc::new(RunTelemetry::new()),
        }
    }

    /// A temporal `maxSustainedWind` observation for `id`, stamped at `observed_at`.
    fn wind_observation(id: &str, value: f64, observed_at: &str) -> NgsiLdEntity {
        let mut property = NgsiLdProperty::new(Value::from(json!(value)));
        property.observed_at = Some(observed_at.parse::<DateTime<Utc>>().unwrap());
        let mut entity = NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("TropicalCyclone").unwrap());
        entity.attributes.insert(
            NameBuf::new("maxSustainedWind").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(property)),
        );
        entity
    }

    /// Feeds a prepared list of signals through the aggregator stage and returns the drained output.
    fn run_stage(inputs: Vec<Signal<Batch<NgsiLdEntity>, PipelineError>>) -> Vec<Signal<Batch<NgsiLdEntity>, PipelineError>> {
        let (tx, rx) = channel(ChannelPolicy::Bounded(NonZeroUsize::new(1024).unwrap()));
        for signal in inputs {
            tx.send(signal).unwrap();
        }
        drop(tx);
        spawn_aggregator_thread(rx, 0, env()).into_iter().collect()
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
}

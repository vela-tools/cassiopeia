use crate::{
    error::PipelineError,
    pipeline_stage::PipelineStage,
    stages::{
        pump::{BatchSender, PumpConfig, PumpProcessor, run_pump_stage},
        stage_env::StageEnv,
    },
};
use cassiopeia_common::{
    batch::Batch,
    channel::ChannelReceiver,
    signal::Signal,
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, component::StageComponent},
};
use cassiopeia_ir::{fragment::Fragment, mapped::Mapped};
use cassiopeia_reporter::guard::StageGuard;
use cassiopeia_resolver::fragment_resolver::FragmentResolver;
use std::ops::ControlFlow;

/// Merges every lane's fragments into the entity store, a batch at a time.
///
/// The batch size is the producer's: the expander hands over groups already sized by the run's
/// configuration, so the resolver amortises its store writes across whatever it is given.
struct ResolverProcessor {
    /// The resolver that merges fragments into the entity and relationship stores.
    resolver: FragmentResolver,
}

impl PumpProcessor for ResolverProcessor {
    type In = Mapped<Fragment>;
    type Out = ();

    fn process(&mut self, batch: Batch<Mapped<Fragment>>, stage: &StageGuard, tx: &BatchSender<()>) -> ControlFlow<()> {
        let service = stage.service_span();
        let (results, timing) = self.resolver.resolve_batch_timed(Vec::from(batch));
        drop(service);

        let mut resolved = 0;
        for result in results {
            match result {
                Ok(()) => resolved += 1,
                Err(error) => {
                    stage.inc_by(resolved);
                    stage.fail(1);
                    let _ = tx.send(Signal::Error(PipelineError::from(error)));
                    return ControlFlow::Break(());
                }
            }
        }
        stage.inc_by(resolved);

        stage.component_time(StageComponent::ResolvePrepare, timing.preparation);
        stage.component_time(StageComponent::RelationshipStoreWrite, timing.relationship_write);
        stage.component_time(StageComponent::EntityStoreWrite, timing.entity_write);
        stage.component_time(StageComponent::StoreWrite, timing.store_write());
        ControlFlow::Continue(())
    }
}

/// Spawns the resolver sink that merges every lane's fragments into the entity store.
///
/// The sink reads from one shared channel fed by all expander lanes, so it counts down
/// `expected_stops` (one `Stop` per lane) before finishing.
pub(crate) fn spawn_resolver_thread(
    resolver: FragmentResolver,
    receiver: ChannelReceiver<Signal<Batch<Mapped<Fragment>>, PipelineError>>,
    expected_stops: usize,
    env: StageEnv,
) -> ChannelReceiver<Signal<Batch<()>, PipelineError>> {
    run_pump_stage(
        ResolverProcessor { resolver },
        receiver,
        PumpConfig {
            stage: PipelineStage::ResolverSink,
            set_length: None,
            expected_stops,
            boundary: ChannelBoundary::to_sink(Stage::Resolver),
            env,
        },
    )
}

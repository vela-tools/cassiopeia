use crate::{
    completion::drain_completion,
    controller::RunController,
    error::Result,
    input_config::InputConfig,
    pipeline::Pipeline,
    stages::{
        collector::spawn_collector_thread,
        expander::spawn_expander_thread,
        ingestor::spawn_ingestor_bank,
        profiler::{create_profiler, spawn_profiler_thread},
        resolver::spawn_resolver_thread,
        stage_env::StageEnv,
    },
};
use cassiopeia_collector::generic::GenericCollector;
use cassiopeia_common::{
    signal::{MetaSignal, Signal},
    stage::Stage,
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_expander::{expander::Expander, generic::GenericExpander, router::MappingRouter};
use cassiopeia_mapping::template::resolver::TemplateResolver;
use cassiopeia_resolver::{entity_source::EntitySource, fragment_resolver::FragmentResolver};
use std::sync::Arc;

impl Pipeline {
    /// Runs every input lane and merges its fragments into the store, returning the number of unique
    /// entities the store holds afterwards.
    ///
    /// # Errors
    ///
    /// Returns the resolver's typed [`PipelineError`](crate::error::PipelineError) when the sink
    /// reports a fragment could not be resolved.
    pub(crate) fn run_phase1(
        &self,
        loaded: Vec<(&InputConfig, MappingRouter)>,
        template_resolver: &TemplateResolver,
        fragment_resolver: &FragmentResolver,
        schedule_meta: &MetaSignal,
        controller: &Arc<dyn RunController>,
        telemetry: Arc<RunTelemetry>,
    ) -> Result<u64> {
        let batch_size = self.config.handoff_size();
        let channel_policy = self.config.channel_policy;
        let num_inputs = loaded.len();
        let env = StageEnv {
            channel_policy,
            reporter: self.context.reporter,
            controller: controller.clone(),
            telemetry,
        };

        // Every lane merges into one channel governed by the selected memory policy; it is the
        // resolver's intake, fed by every lane's expander bridge.
        let (shared_tx, shared_rx) = env.channel(Some(ChannelBoundary::between(Stage::Expander, Stage::Resolver)));
        let _ = shared_tx.send(Signal::Meta(schedule_meta.clone()));

        for (input, router) in loaded {
            let ingestor_bank = spawn_ingestor_bank(batch_size, channel_policy, self.context.reporter, &env.telemetry);

            let collector = Box::new(GenericCollector::new(
                input.collector_source.clone(),
                input.format_override,
                self.config.default_user_agent.clone(),
            ));
            let collector_rx = spawn_collector_thread(collector, channel_policy, Arc::clone(&env.telemetry));

            let profiler = create_profiler(
                input.format_override,
                collector_rx,
                channel_policy,
                shared_tx.clone(),
                self.context.reporter,
                Arc::clone(&env.telemetry),
            );
            spawn_profiler_thread(profiler, ingestor_bank.routes, shared_tx.clone(), Arc::clone(&env.telemetry));

            let expander: Box<dyn Expander> = Box::new(GenericExpander::new(router, template_resolver.clone(), input.vars.clone()));
            // Each lane's expander writes its fragments straight onto the shared resolver intake, so
            // no per-lane channel or bridge thread sits between the expander and the resolver.
            spawn_expander_thread(expander, ingestor_bank.output_rx, shared_tx.clone(), batch_size, &env);
        }

        // Drop the original sender so the shared channel closes once every lane bridge finishes.
        drop(shared_tx);

        let resolver_rx = spawn_resolver_thread(fragment_resolver.clone(), shared_rx, num_inputs, env);

        drain_completion(resolver_rx)?;

        Ok(fragment_resolver.get_unique_entity_count().unwrap_or(0))
    }
}

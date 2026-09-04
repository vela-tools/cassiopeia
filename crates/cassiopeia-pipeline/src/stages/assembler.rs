use crate::{error::PipelineError, pipeline_stage::PipelineStage, stages::stage_env::StageEnv};
use cassiopeia_common::{
    batch::Batch,
    channel::{ChannelReceiver, ChannelSender},
    signal::Signal,
    stage::Stage,
    telemetry::channel_boundary::ChannelBoundary,
};
use cassiopeia_ir::assembled_entity::AssembledEntity;
use cassiopeia_reporter::guard::StageGuard;
use cassiopeia_resolver::{entity_source::EntitySource, error::ResolverError};
use execution_time::ExecutionTime;
use std::{mem, ops::ControlFlow, thread::spawn};

/// The assembled-entity channel produced by the assembler stage.
type AssembledSender = ChannelSender<Signal<Batch<AssembledEntity>, PipelineError>>;

/// Accumulates assembled units and hands them downstream a batch at a time.
///
/// The store scan produces one unit at a time, but the extractor wants groups, so this buffers into
/// a batch of the configured size and flushes when it fills. Units keep their order, which is what
/// lets the downstream fold group an id's observations while holding one id at a time.
struct BatchEmitter<'a> {
    tx: &'a AssembledSender,
    batch_size: usize,
    pending: Batch<AssembledEntity>,
}

impl<'a> BatchEmitter<'a> {
    /// Creates an emitter that flushes every `batch_size` units.
    fn new(tx: &'a AssembledSender, batch_size: usize) -> BatchEmitter<'a> {
        BatchEmitter {
            tx,
            batch_size,
            pending: Batch::with_capacity(batch_size),
        }
    }

    /// Accepts one assembled unit, flushing once the batch is full.
    fn accept(&mut self, entity: AssembledEntity, stage: &StageGuard) -> ControlFlow<()> {
        self.pending.push(entity);
        if self.pending.len() >= self.batch_size {
            return self.flush(stage);
        }
        ControlFlow::Continue(())
    }

    /// Hands whatever has accumulated downstream, counting it once.
    fn flush(&mut self, stage: &StageGuard) -> ControlFlow<()> {
        if self.pending.is_empty() {
            return ControlFlow::Continue(());
        }
        let batch = mem::replace(&mut self.pending, Batch::with_capacity(self.batch_size));
        let count = batch.count();
        stage.received(count);
        if stage.measure_output_wait(|| self.tx.send(Signal::Data(batch)).is_ok()) {
            stage.inc_by(count);
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}

/// Spawns the assembler, the phase-2 source of the entity stream.
///
/// It scans the resolver's entity store and assembles each id's stored fragments into
/// [`AssembledEntity`] emit-units, forwarding them to the extractor in batches. Assembly runs
/// parallel per chunk inside the resolver, but the thread and the channel plumbing live here so the
/// work is measured as its own stage. Each base id's units are emitted contiguously, and the stores
/// are destroyed once the scan finishes.
pub(crate) fn spawn_assembler_thread(
    resolver: Box<dyn EntitySource>,
    entity_count: u64,
    batch_size: usize,
    env: &StageEnv,
) -> ChannelReceiver<Signal<Batch<AssembledEntity>, PipelineError>> {
    let reporter = env.reporter;
    let controller = env.controller.clone();
    let run_telemetry = env.telemetry.clone();
    let (tx, rx) = env.channel(Some(ChannelBoundary::between(Stage::Assembler, Stage::Extractor)));

    spawn(move || {
        let execution_time = ExecutionTime::start();
        let stage = reporter
            .enter_stage(Box::new(PipelineStage::Assembler), execution_time)
            .with_telemetry(run_telemetry.start_stage(Stage::Assembler));
        stage.set_length(entity_count);
        let _ = tx.send(Signal::Start);

        // The `emit` closure borrows `tx`, so the scan runs in a block that ends the borrow before the
        // closing `Stop` is sent.
        let assembly = {
            let mut emitter = BatchEmitter::new(&tx, batch_size);
            let assembly = {
                let mut emit = |result: Result<AssembledEntity, ResolverError>| -> ControlFlow<()> {
                    if controller.should_cancel() {
                        return ControlFlow::Break(());
                    }
                    match result {
                        Ok(entity) => emitter.accept(entity, &stage),
                        Err(error) => {
                            if tx.send(Signal::Error(PipelineError::from(error))).is_ok() {
                                ControlFlow::Continue(())
                            } else {
                                ControlFlow::Break(())
                            }
                        }
                    }
                };
                resolver.drive_assembly(batch_size, &mut emit)
            };
            // The scan ends mid-batch whenever the unit count is not a multiple of the batch size.
            let _ = emitter.flush(&stage);
            assembly
        };
        match assembly {
            // The chunks ran on rayon workers, so this is their wall time as the driving thread saw
            // it; the stage's CPU column stays empty because a per-thread clock here cannot see them.
            Ok(timing) => stage.add_service_time(timing.assembly),
            Err(error) => {
                let _ = tx.send(Signal::Error(PipelineError::from(error)));
            }
        }

        resolver.destroy();
        let _ = tx.send(Signal::Stop);
    });

    rx
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::RunController,
        error::PipelineError,
        stages::{assembler::spawn_assembler_thread, stage_env::StageEnv},
    };
    use cassiopeia_common::{batch::Batch, channel::ChannelPolicy, signal::Signal, telemetry::run::RunTelemetry};
    use cassiopeia_ir::{assembled_entity::AssembledEntity, fragment::Fragment, mapped::Mapped};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_reporter::{backend::noop::NoopReporter, reporter::Reporter};
    use cassiopeia_resolver::{
        entity_store::dashmap_latest_store::DashMapLatestEntityStore,
        fragment_resolver::FragmentResolver,
        fragment_sink::FragmentSink,
        relationship_store::dashmap_store::DashMapRelationshipStore,
    };
    use serde_json::json;
    use std::{path::Path, sync::Arc};

    static NOOP: NoopReporter = NoopReporter::new();

    /// A controller that never cancels, so the assembler runs to completion.
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

    /// Builds a resolver holding one current-state fragment per id for each of `ids`.
    fn resolver_with(ids: &[&str]) -> FragmentResolver {
        let mut runner = TemplateRunner::new();
        let document = r#"{ version: "v4", dataModel: "AirQualityObserved", identity: { entityName: "Station-{{ id }}" }, attributes: { temperature: { source: "{{ temperature }}" } } }"#;
        let mapping = Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap());
        let resolver = runner.resolver();
        let fragment_resolver = FragmentResolver::new(Box::new(DashMapLatestEntityStore::new()), Box::new(DashMapRelationshipStore::new()), resolver);

        let sink: &dyn FragmentSink = &fragment_resolver;
        for id in ids {
            let fragment = Fragment::new(json!({ "temperature": 20 }), id.parse().unwrap(), None, None);
            sink.resolve(Mapped::new(fragment, Arc::clone(&mapping))).unwrap();
        }
        fragment_resolver
    }

    /// Every assembled id that reached the output, sorted for a stable assertion.
    fn assembled_ids(output: &[Signal<Batch<AssembledEntity>, PipelineError>]) -> Vec<String> {
        let mut ids: Vec<String> = output
            .iter()
            .filter_map(|signal| match signal {
                Signal::Data(batch) => Some(batch.iter().map(|entity| entity.id().to_string())),
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => None,
            })
            .flatten()
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn the_stage_brackets_one_assembled_unit_per_id_with_start_and_stop() {
        let resolver = resolver_with(&["urn:ngsi-ld:Station:1", "urn:ngsi-ld:Station:2"]);

        let rx = spawn_assembler_thread(Box::new(resolver), 2, 8, &env());
        let output: Vec<Signal<Batch<AssembledEntity>, PipelineError>> = rx.into_iter().collect();

        assert!(matches!(output.first(), Some(Signal::Start)));
        assert!(matches!(output.last(), Some(Signal::Stop)));
        assert_eq!(
            assembled_ids(&output),
            vec!["urn:ngsi-ld:Station:1".to_string(), "urn:ngsi-ld:Station:2".to_string()]
        );
    }

    #[test]
    fn a_final_partial_batch_is_still_flushed() {
        // Five ids with a batch size of two ends mid-batch, so the last unit only reaches the
        // extractor if the scan flushes what it has left.
        let resolver = resolver_with(&[
            "urn:ngsi-ld:Station:1",
            "urn:ngsi-ld:Station:2",
            "urn:ngsi-ld:Station:3",
            "urn:ngsi-ld:Station:4",
            "urn:ngsi-ld:Station:5",
        ]);

        let rx = spawn_assembler_thread(Box::new(resolver), 5, 2, &env());
        let output: Vec<Signal<Batch<AssembledEntity>, PipelineError>> = rx.into_iter().collect();

        assert_eq!(assembled_ids(&output).len(), 5);
    }
}

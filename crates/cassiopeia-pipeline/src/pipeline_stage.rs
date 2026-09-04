use cassiopeia_common::stage::Stage;
use cassiopeia_reporter::{
    reporter::ProgressStage,
    stage_id::{StageId, StageLabel},
};
use std::fmt::{self, Display, Formatter};

/// The stages a pipeline run passes through, in order, for progress reporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PipelineStage {
    /// Decides when a run fires.
    Scheduler,
    /// Reads sources into collected payloads.
    Collector,
    /// Detects each payload's format.
    Profiler,
    /// Parses payloads into records.
    Ingestor,
    /// Turns records into mapped fragments.
    Expander,
    /// Merges fragments into the entity store.
    ResolverSink,
    /// Scans the store and assembles each id's fragments into entities.
    Assembler,
    /// Resolves each assembled entity's attribute values.
    Extractor,
    /// Builds NGSI-LD entities.
    Transformer,
    /// Checks entities against their schemas.
    Validator,
    /// Folds each id's observations into one temporal entity (series runs only).
    Aggregator,
    /// Emits entities to the destination.
    Writer,
}

impl PipelineStage {
    /// The stages that run for every cycle, in display order. Excludes [`PipelineStage::Scheduler`],
    /// which brackets whole runs rather than participating in one, and [`PipelineStage::Aggregator`],
    /// which the composition root splices in before the writer only for a series run.
    #[must_use]
    pub const fn all() -> &'static [PipelineStage] {
        &[
            PipelineStage::Collector,
            PipelineStage::Profiler,
            PipelineStage::Ingestor,
            PipelineStage::Expander,
            PipelineStage::ResolverSink,
            PipelineStage::Assembler,
            PipelineStage::Extractor,
            PipelineStage::Transformer,
            PipelineStage::Validator,
            PipelineStage::Writer,
        ]
    }

    /// Maps this progress stage to its telemetry [`Stage`], if it measures one.
    ///
    /// [`PipelineStage::Scheduler`] brackets whole runs rather than measuring a stage, so it maps to
    /// `None`; every other variant maps to the single typed identity telemetry keys on.
    #[must_use]
    pub const fn stage(self) -> Option<Stage> {
        match self {
            PipelineStage::Scheduler => None,
            PipelineStage::Collector => Some(Stage::Collector),
            PipelineStage::Profiler => Some(Stage::Profiler),
            PipelineStage::Ingestor => Some(Stage::Ingestor),
            PipelineStage::Expander => Some(Stage::Expander),
            PipelineStage::ResolverSink => Some(Stage::Resolver),
            PipelineStage::Assembler => Some(Stage::Assembler),
            PipelineStage::Extractor => Some(Stage::Extractor),
            PipelineStage::Transformer => Some(Stage::Transformer),
            PipelineStage::Validator => Some(Stage::Validator),
            PipelineStage::Aggregator => Some(Stage::Aggregator),
            PipelineStage::Writer => Some(Stage::Writer),
        }
    }
}

impl Display for PipelineStage {
    /// Writes the stage's human-readable label, the same one the reporter shows.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label().as_str())
    }
}

impl ProgressStage for PipelineStage {
    fn id(&self) -> StageId {
        match self.stage() {
            // The id derives from the single `Stage` identity, so no parallel string table exists.
            Some(stage) => StageId::new(stage.id()),
            None => StageId::new("scheduler"),
        }
    }

    fn label(&self) -> StageLabel {
        match self.stage() {
            Some(stage) => StageLabel::new(stage.label()),
            None => StageLabel::new("Scheduler"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline_stage::PipelineStage;
    use cassiopeia_reporter::reporter::ProgressStage;

    #[test]
    fn the_run_stages_exclude_the_scheduler() {
        assert!(!PipelineStage::all().contains(&PipelineStage::Scheduler));
        assert_eq!(PipelineStage::all().len(), 10);
        assert_eq!(PipelineStage::all().first(), Some(&PipelineStage::Collector));
        assert_eq!(PipelineStage::all().last(), Some(&PipelineStage::Writer));
    }

    #[test]
    fn every_stage_ids_and_labels_the_way_the_reporter_expects() {
        assert_eq!(PipelineStage::ResolverSink.id().as_str(), "resolver");
        assert_eq!(PipelineStage::ResolverSink.label().as_str(), "Resolver");
        assert_eq!(PipelineStage::Assembler.id().as_str(), "assembler");
        assert_eq!(PipelineStage::Assembler.label().as_str(), "Assembler");
        assert_eq!(PipelineStage::Extractor.id().as_str(), "extractor");
        assert_eq!(PipelineStage::Extractor.label().as_str(), "Extractor");
        assert_eq!(PipelineStage::Collector.id().as_str(), "collector");
        assert_eq!(PipelineStage::Collector.label().as_str(), "Collector");
    }

    #[test]
    fn a_stage_displays_as_its_label() {
        assert_eq!(PipelineStage::Ingestor.to_string(), "Ingestor");
        assert_eq!(PipelineStage::ResolverSink.to_string(), "Resolver");
    }

    #[test]
    fn the_scheduler_maps_to_no_measured_stage_while_the_rest_map_to_one() {
        use cassiopeia_common::stage::Stage;

        assert_eq!(PipelineStage::Scheduler.stage(), None);
        assert_eq!(PipelineStage::ResolverSink.stage(), Some(Stage::Resolver));
        assert_eq!(PipelineStage::Extractor.stage(), Some(Stage::Extractor));
        for stage in PipelineStage::all() {
            assert!(stage.stage().is_some());
        }
    }
}

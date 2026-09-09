use cassiopeia_common::{
    channel::ChannelPolicy,
    context::mode::AtContextMode,
    memory_profile::MemoryProfile,
    pipeline_mode::PipelineMode,
    store_kind::StoreKind,
    user_agent::UserAgent,
};
use std::{num::NonZeroUsize, path::PathBuf};

/// Whether entity extraction fans a batch across threads.
///
/// Only the extractor honours this engine knob; the other stages follow the run [`PipelineMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionParallelism {
    /// Extract a batch across a Rayon thread pool.
    #[default]
    Parallel,
    /// Extract on the calling thread.
    Sequential,
}

/// The engine settings a run is executed with, distinct from the manifest that describes *what* to
/// run.
///
/// Destination, representation overrides, `@context` mode, and tenant are read from the manifest;
/// this struct carries only the knobs that tune how the pipeline itself executes: batch size,
/// channel backpressure, store backends, and the memory profile.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// How many records or entities a batch stage buffers before flushing.
    pub batch_size: usize,
    /// Whether the extractor runs a batch in parallel.
    pub extraction: ExtractionParallelism,
    /// Whether stages process record-by-record or in batches.
    pub mode: PipelineMode,
    /// The backend for the entity store.
    pub entity_store: StoreKind,
    /// The backend for the relationship store.
    pub relationship_store: StoreKind,
    /// Where JSON Schema files and the Smart Data Models catalog live.
    pub schemas_folder: PathBuf,
    /// How the `@context` is resolved, when the manifest output does not state its own mode.
    pub context_mode: AtContextMode,
    /// The `User-Agent` a remote source is fetched with, and the one the broker writer sends when
    /// the manifest destination names none. Supplied by the composition root as the build-time
    /// default, not read from configuration.
    pub default_user_agent: UserAgent,
    /// Whether stage handoffs favour unrestricted overlap or bounded in-flight memory.
    pub channel_policy: ChannelPolicy,
    /// The pre-tuned parameter set to apply.
    pub memory_profile: MemoryProfile,
}

impl PipelineConfig {
    /// The number of items a stage hands to the next in one message.
    ///
    /// [`PipelineMode::Single`] is exactly a batch of one. It stays a user-facing mode, but it is not
    /// a second code path: every stage batches, and single mode simply sets that batch to one item.
    #[must_use]
    pub const fn handoff_size(&self) -> usize {
        match self.mode {
            PipelineMode::Single => 1,
            PipelineMode::Batch => self.batch_size,
        }
    }

    /// Overrides the fields the memory profile governs.
    ///
    /// [`MemoryProfile::Default`] is a no-op; [`MemoryProfile::LowMemory`] forces the tight-footprint
    /// combination: on-disk stores, sequential extraction, small batches, and a shallow channel.
    pub const fn apply_memory_profile(&mut self) {
        match self.memory_profile {
            MemoryProfile::Default => {}
            MemoryProfile::LowMemory => {
                self.batch_size = 2000;
                self.entity_store = StoreKind::Redb;
                self.relationship_store = StoreKind::Redb;
                self.extraction = ExtractionParallelism::Sequential;
                self.channel_policy = ChannelPolicy::Bounded(NonZeroUsize::MIN.saturating_add(3));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline_config::{ExtractionParallelism, PipelineConfig};
    use cassiopeia_common::{
        channel::ChannelPolicy,
        context::mode::AtContextMode,
        memory_profile::MemoryProfile,
        pipeline_mode::PipelineMode,
        store_kind::StoreKind,
        user_agent::UserAgent,
    };
    use std::{num::NonZeroUsize, path::PathBuf};

    fn config(memory_profile: MemoryProfile) -> PipelineConfig {
        PipelineConfig {
            batch_size: 10_000,
            extraction: ExtractionParallelism::Parallel,
            mode: PipelineMode::Batch,
            entity_store: StoreKind::DashMap,
            relationship_store: StoreKind::DashMap,
            schemas_folder: PathBuf::from("schemas"),
            context_mode: AtContextMode::Default,
            default_user_agent: UserAgent::from("test".to_owned()),
            channel_policy: ChannelPolicy::Bounded(NonZeroUsize::new(64).unwrap()),
            memory_profile,
        }
    }

    #[test]
    fn the_default_profile_changes_nothing() {
        let mut config = config(MemoryProfile::Default);
        config.apply_memory_profile();

        assert_eq!(config.batch_size, 10_000);
        assert_eq!(config.entity_store, StoreKind::DashMap);
        assert_eq!(config.extraction, ExtractionParallelism::Parallel);
        assert_eq!(config.channel_policy, ChannelPolicy::Bounded(NonZeroUsize::new(64).unwrap()));
    }

    #[test]
    fn the_low_memory_profile_forces_the_tight_footprint_combination() {
        let mut config = config(MemoryProfile::LowMemory);
        config.apply_memory_profile();

        assert_eq!(config.batch_size, 2000);
        assert_eq!(config.entity_store, StoreKind::Redb);
        assert_eq!(config.relationship_store, StoreKind::Redb);
        assert_eq!(config.extraction, ExtractionParallelism::Sequential);
        assert_eq!(config.channel_policy, ChannelPolicy::Bounded(NonZeroUsize::new(4).unwrap()));
    }
}

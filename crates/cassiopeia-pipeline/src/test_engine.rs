use crate::pipeline_config::{ExtractionParallelism, PipelineConfig};
use cassiopeia_common::{
    channel::ChannelPolicy,
    context::mode::AtContextMode,
    memory_profile::MemoryProfile,
    pipeline_mode::PipelineMode,
    store_kind::StoreKind,
    user_agent::UserAgent,
};
use std::{num::NonZeroUsize, path::PathBuf, sync::atomic::AtomicBool};

/// The shutdown flag every test pipeline runs under, which no test ever raises.
pub(crate) static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// The engine knobs a test pipeline runs under; the values match the crate defaults, and a test
/// overrides the ones it exercises.
pub(crate) fn engine_config() -> PipelineConfig {
    PipelineConfig {
        batch_size: 10_000,
        extraction: ExtractionParallelism::Parallel,
        mode: PipelineMode::Batch,
        entity_store: StoreKind::DashMap,
        relationship_store: StoreKind::DashMap,
        schemas_folder: PathBuf::from("schemas"),
        cursors_folder: PathBuf::from("cursors"),
        context_mode: AtContextMode::Default,
        default_user_agent: UserAgent::from("test".to_owned()),
        channel_policy: ChannelPolicy::Bounded(NonZeroUsize::new(64).unwrap()),
        memory_profile: MemoryProfile::Default,
    }
}

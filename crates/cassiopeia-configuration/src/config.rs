use crate::{
    download::Download,
    mappings::Mappings,
    pipeline::{ExtractionMode, Pipeline},
    resolver::Resolver,
    schemas::Schemas,
};
use cassiopeia_common::{log::config::LoggerConfig, memory_profile::MemoryProfile, store_kind::StoreKind};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

// Both counts are built up from `NonZeroUsize::MIN` so the compiler proves them non-zero without
// an unwrap at the constant.
/// The batch size a low-memory run uses, whatever the pipeline section says.
const LOW_MEMORY_BATCH_SIZE: NonZeroUsize = NonZeroUsize::MIN.saturating_add(1_999);
/// The channel capacity a low-memory run uses, whatever the pipeline section says.
const LOW_MEMORY_CHANNEL_CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(3);

/// Everything an installation can configure, section by section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// How remote sources are fetched.
    pub download: Download,

    /// Where log records go.
    pub logger: LoggerConfig,

    /// Where JSON Schemas are kept.
    pub schemas: Schemas,

    /// How records move between pipeline stages.
    pub pipeline: Pipeline,

    /// Where mapping documents are kept.
    pub mappings: Mappings,

    /// Where entity resolution keeps its state.
    pub resolver: Resolver,
}

impl Config {
    /// Rewrites the sections a memory profile governs so they agree with it.
    ///
    /// `LowMemory` is a statement about the host, not a preference, so it overrides whatever the
    /// pipeline and resolver sections asked for: smaller batches, a shorter channel, sequential
    /// extraction, and disk-backed resolution state. Applied once after every layer has been
    /// merged, so a profile set in any layer governs values set in any other.
    pub const fn apply_memory_profile(&mut self) {
        match self.pipeline.memory_profile {
            MemoryProfile::Default => {}
            MemoryProfile::LowMemory => {
                self.pipeline.batch_size = LOW_MEMORY_BATCH_SIZE;
                self.pipeline.channel_capacity = Some(LOW_MEMORY_CHANNEL_CAPACITY);
                self.pipeline.extraction_mode = ExtractionMode::Sequential;
                self.resolver.entity_store = StoreKind::Redb;
                self.resolver.relationship_store = StoreKind::Redb;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::Config, pipeline::ExtractionMode};
    use cassiopeia_common::{memory_profile::MemoryProfile, store_kind::StoreKind};

    #[test]
    fn the_default_profile_leaves_every_section_alone() {
        let mut config = Config::default();
        let untouched = config.clone();

        config.apply_memory_profile();

        assert_eq!(config, untouched);
    }

    #[test]
    fn the_low_memory_profile_overrides_what_the_pipeline_section_asked_for() {
        let mut config = Config::default();
        config.pipeline.memory_profile = MemoryProfile::LowMemory;

        config.apply_memory_profile();

        assert_eq!(config.pipeline.batch_size.get(), 2_000);
        assert_eq!(config.pipeline.channel_capacity.unwrap().get(), 4);
        assert_eq!(config.pipeline.extraction_mode, ExtractionMode::Sequential);
        assert_eq!(config.resolver.entity_store, StoreKind::Redb);
        assert_eq!(config.resolver.relationship_store, StoreKind::Redb);
    }

    #[test]
    fn an_omitted_section_falls_back_to_its_own_defaults() {
        let config: Config = toml::from_str("[pipeline]\nbatch_size = 5").unwrap();

        assert_eq!(config.pipeline.batch_size.get(), 5);
        assert_eq!(config.resolver, Config::default().resolver);
    }
}

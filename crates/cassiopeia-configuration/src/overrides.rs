use cassiopeia_common::{memory_profile::MemoryProfile, store_kind::StoreKind};
use figment2::{Figment, providers::Serialized};
use std::num::NonZeroUsize;

/// A single configured value replaced for one invocation.
///
/// Each variant names exactly the setting it replaces, so an invocation only displaces the values
/// it was actually given rather than a whole section reconstructed around them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigOverride {
    /// Replaces `resolver.entity_store`.
    EntityStore(StoreKind),

    /// Replaces `resolver.relationship_store`.
    RelationshipStore(StoreKind),

    /// Replaces `pipeline.memory_profile`.
    MemoryProfile(MemoryProfile),

    /// Replaces `pipeline.batch_size`.
    BatchSize(NonZeroUsize),

    /// Replaces `pipeline.channel_capacity`; `None` selects unbounded handoffs.
    ChannelCapacity(Option<NonZeroUsize>),

    /// Replaces `pipeline.worker_threads`.
    WorkerThreads(NonZeroUsize),
}

impl ConfigOverride {
    /// Layers this override over everything merged into `figment` so far.
    #[must_use]
    pub fn apply(&self, figment: Figment) -> Figment {
        match self {
            ConfigOverride::EntityStore(store) => figment.merge(Serialized::global("resolver.entity_store", store)),
            ConfigOverride::RelationshipStore(store) => figment.merge(Serialized::global("resolver.relationship_store", store)),
            ConfigOverride::MemoryProfile(profile) => figment.merge(Serialized::global("pipeline.memory_profile", profile)),
            ConfigOverride::BatchSize(size) => figment.merge(Serialized::global("pipeline.batch_size", size)),
            ConfigOverride::ChannelCapacity(capacity) => figment.merge(Serialized::global("pipeline.channel_capacity", capacity)),
            ConfigOverride::WorkerThreads(threads) => figment.merge(Serialized::global("pipeline.worker_threads", threads)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::Config, overrides::ConfigOverride};
    use cassiopeia_common::{memory_profile::MemoryProfile, store_kind::StoreKind};
    use figment2::{Figment, providers::Serialized};
    use std::num::NonZeroUsize;

    fn config_with(override_value: &ConfigOverride) -> Config {
        let figment = Figment::from(Serialized::defaults(Config::default()));
        override_value.apply(figment).extract().unwrap()
    }

    #[test]
    fn an_entity_store_override_displaces_only_the_entity_store() {
        let config = config_with(&ConfigOverride::EntityStore(StoreKind::Redb));

        assert_eq!(config.resolver.entity_store, StoreKind::Redb);
        assert_eq!(config.resolver.relationship_store, Config::default().resolver.relationship_store);
    }

    #[test]
    fn a_memory_profile_override_displaces_only_the_pipeline_profile() {
        let config = config_with(&ConfigOverride::MemoryProfile(MemoryProfile::LowMemory));

        assert_eq!(config.pipeline.memory_profile, MemoryProfile::LowMemory);
    }

    #[test]
    fn each_engine_knob_displaces_only_the_setting_it_names() {
        let batch = config_with(&ConfigOverride::BatchSize(NonZeroUsize::new(500).unwrap()));
        assert_eq!(batch.pipeline.batch_size.get(), 500);
        assert_eq!(batch.pipeline.channel_capacity, None);
        assert_eq!(batch.pipeline.worker_threads, None);

        let threads = config_with(&ConfigOverride::WorkerThreads(NonZeroUsize::new(6).unwrap()));
        assert_eq!(threads.pipeline.worker_threads.map(NonZeroUsize::get), Some(6));
        assert_eq!(threads.pipeline.batch_size, Config::default().pipeline.batch_size);
    }

    #[test]
    fn a_channel_capacity_override_can_select_bounded_or_unbounded_handoffs() {
        let bounded = config_with(&ConfigOverride::ChannelCapacity(NonZeroUsize::new(16)));
        assert_eq!(bounded.pipeline.channel_capacity.map(NonZeroUsize::get), Some(16));

        // An explicit `none` has to be able to override a configured capacity back to unbounded.
        let unbounded = config_with(&ConfigOverride::ChannelCapacity(None));
        assert_eq!(unbounded.pipeline.channel_capacity, None);
    }
}

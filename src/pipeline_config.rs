use cassiopeia_common::{channel::ChannelPolicy, context::mode::AtContextMode};
use cassiopeia_configuration::{config::Config, pipeline::ExtractionMode};
use cassiopeia_pipeline::pipeline_config::{ExtractionParallelism, PipelineConfig};

/// Translates the loaded configuration into the pipeline engine's own configuration.
///
/// This is composition-root adapter glue between two sibling libraries (the configuration-loading
/// layer and the pipeline engine) rather than domain assembly, so it lives at the binary that
/// composes them instead of inside either library.
///
/// The writer's representation, skip-null, target, tenant, and `@context` mode are read from the
/// manifest, so `context_mode` is only the fallback used when the manifest output does not set one;
/// [`AtContextMode::None`] keeps a manifest that declares no `@context` from acquiring the default.
pub fn pipeline_config(config: &Config) -> PipelineConfig {
    PipelineConfig {
        batch_size: config.pipeline.batch_size.get(),
        extraction: extraction_parallelism(config.pipeline.extraction_mode),
        mode: config.pipeline.mode,
        entity_store: config.resolver.entity_store,
        relationship_store: config.resolver.relationship_store,
        schemas_folder: config.schemas.folder.clone(),
        context_mode: AtContextMode::None,
        // The default user-agent has no configuration source; the composition root stamps the
        // build-time value onto this field, mirroring how it stamps the run `mode`.
        default_user_agent: String::new(),
        channel_policy: match config.pipeline.channel_capacity {
            Some(capacity) => ChannelPolicy::Bounded(capacity),
            None => ChannelPolicy::Unbounded,
        },
        memory_profile: config.pipeline.memory_profile,
    }
}

/// Maps the configuration's extraction mode onto the pipeline's own parallelism enum.
const fn extraction_parallelism(mode: ExtractionMode) -> ExtractionParallelism {
    match mode {
        ExtractionMode::Parallel => ExtractionParallelism::Parallel,
        ExtractionMode::Sequential => ExtractionParallelism::Sequential,
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline_config::{extraction_parallelism, pipeline_config};
    use cassiopeia_common::{channel::ChannelPolicy, context::mode::AtContextMode};
    use cassiopeia_configuration::{config::Config, pipeline::ExtractionMode};
    use cassiopeia_pipeline::pipeline_config::ExtractionParallelism;
    use std::num::NonZeroUsize;

    #[test]
    fn parallel_extraction_maps_to_the_parallel_variant() {
        assert_eq!(extraction_parallelism(ExtractionMode::Parallel), ExtractionParallelism::Parallel);
    }

    #[test]
    fn sequential_extraction_maps_to_the_sequential_variant() {
        assert_eq!(extraction_parallelism(ExtractionMode::Sequential), ExtractionParallelism::Sequential);
    }

    #[test]
    fn the_context_mode_starts_unset_so_the_manifest_governs_it() {
        let config = Config::default();

        assert_eq!(pipeline_config(&config).context_mode, AtContextMode::None);
    }

    #[test]
    fn the_default_user_agent_is_left_empty_for_the_composition_root_to_stamp() {
        let config = Config::default();

        assert!(pipeline_config(&config).default_user_agent.is_empty());
    }

    #[test]
    fn the_batch_size_and_channel_policy_carry_the_configured_values() {
        let config = Config::default();
        let translated = pipeline_config(&config);

        assert_eq!(translated.batch_size, config.pipeline.batch_size.get());
        assert_eq!(translated.channel_policy, ChannelPolicy::Unbounded);
    }

    #[test]
    fn an_explicit_channel_capacity_selects_bounded_back_pressure() {
        let mut config = Config::default();
        let capacity = NonZeroUsize::new(12).unwrap();
        config.pipeline.channel_capacity = Some(capacity);

        assert_eq!(pipeline_config(&config).channel_policy, ChannelPolicy::Bounded(capacity));
    }
}

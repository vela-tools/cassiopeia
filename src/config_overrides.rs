//! Routing principle (the engine-tuning half): a flag lands here only when it tunes the *engine*
//! that executes a run rather than describing the run itself. The store backends and the memory
//! profile configure the pipeline. Each becomes a [`ConfigOverride`] that displaces exactly the
//! configured value it names. Everything that shapes *what* the run does (its output destination,
//! output representation/skip-null, `@context`, validation (mode, schema, representation, skip-null,
//! report), and schedule) is written into the manifest by `manifest_build` instead, never here.
//! The two surfaces are kept disjoint on purpose.

use crate::cli::{Commands, map::MapArgs};
use cassiopeia_common::{channel::ChannelPolicy, memory_profile::MemoryProfile};
use cassiopeia_configuration::overrides::ConfigOverride;

/// Collects the configuration overrides a command's flags imply.
///
/// Only the `map` command carries flags that override configuration; the writer's representation,
/// tenant, broker URL, user-agent, and `@context` mode are carried by the manifest instead, since
/// the pipeline reads them from the manifest destination, not from configuration.
pub fn overrides_from_cli(command: &Commands) -> Vec<ConfigOverride> {
    match command {
        Commands::Map(args) => overrides_from_map(args),
        Commands::Sdm(_)
        | Commands::Mapping(_)
        | Commands::Schema(_)
        | Commands::Manifest(_)
        | Commands::Config(_)
        | Commands::Explorer { .. }
        | Commands::Wizard { .. }
        | Commands::Profile { .. }
        | Commands::Bugreport
        | Commands::Markdown => Vec::new(),
    }
}

/// Collects the overrides the `map` command's flags imply.
fn overrides_from_map(args: &MapArgs) -> Vec<ConfigOverride> {
    let mut overrides = Vec::new();

    if let Some(store) = args.entity_store {
        overrides.push(ConfigOverride::EntityStore(store));
    }
    if let Some(store) = args.relationship_store {
        overrides.push(ConfigOverride::RelationshipStore(store));
    }
    if args.low_memory {
        overrides.push(ConfigOverride::MemoryProfile(MemoryProfile::LowMemory));
    }
    if let Some(size) = args.batch_size {
        overrides.push(ConfigOverride::BatchSize(size));
    }
    if let Some(policy) = args.channel_capacity {
        // The configuration models the policy as an optional bound, so the unbounded choice is the
        // absent one there.
        overrides.push(ConfigOverride::ChannelCapacity(match policy {
            ChannelPolicy::Unbounded => None,
            ChannelPolicy::Bounded(capacity) => Some(capacity),
        }));
    }
    if let Some(threads) = args.threads {
        overrides.push(ConfigOverride::WorkerThreads(threads));
    }

    overrides
}

#[cfg(test)]
mod tests {
    use crate::{cli::Cli, config_overrides::overrides_from_cli};
    use cassiopeia_configuration::overrides::ConfigOverride;
    use clap::Parser;

    /// A minimal `map` command line with the given engine/run flags appended.
    fn overrides_for(extra: &[&str]) -> Vec<ConfigOverride> {
        let mut command_line = vec!["cassiopeia", "map", "-i", "data.csv", "-m", "map.json5"];
        command_line.extend_from_slice(extra);
        let cli = Cli::try_parse_from(command_line).expect("the command line parses");

        overrides_from_cli(&cli.command)
    }

    #[test]
    fn an_engine_tuning_flag_becomes_a_config_override() {
        let overrides = overrides_for(&["--entity-store", "redb"]);

        assert!(overrides.iter().any(|value| matches!(value, ConfigOverride::EntityStore(_))));
    }

    #[test]
    fn a_run_shaping_flag_produces_no_config_override() {
        // `--validation-representation` shapes the run's validation, so it rides the manifest, never
        // a configuration override.
        let overrides = overrides_for(&["--validation-representation", "concise"]);

        assert!(overrides.is_empty());
    }
}

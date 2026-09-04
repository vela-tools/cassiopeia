use crate::{
    config::Config,
    error::{ConfigError, Result},
    overrides::ConfigOverride,
};
use figment2::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use std::path::{Path, PathBuf};

/// The prefix an environment variable needs before it is read as configuration.
const ENV_PREFIX: &str = "CASSIOPEIA_";
/// What separates one section from the next inside an environment variable name.
const ENV_SEPARATOR: &str = "__";

/// Assembles a `Config` from its layers, each one overriding the last.
///
/// The order is fixed: built-in defaults, then the configuration file, then the environment, then
/// the overrides given for this invocation.
#[derive(Debug, Default)]
pub struct ConfigLoader {
    file: Option<PathBuf>,
    overrides: Vec<ConfigOverride>,
}

impl ConfigLoader {
    /// Starts a loader that reads only the built-in defaults and the environment.
    #[must_use]
    pub fn new() -> ConfigLoader {
        ConfigLoader::default()
    }

    /// Reads the configuration file at `path`, which must exist.
    #[must_use]
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> ConfigLoader {
        self.file = Some(path.into());
        self
    }

    /// Adds an override that displaces whatever the earlier layers set.
    #[must_use]
    pub fn with_override(mut self, config_override: ConfigOverride) -> ConfigLoader {
        self.overrides.push(config_override);
        self
    }

    /// Merges every layer and resolves the memory profile against the result.
    ///
    /// # Errors
    /// Returns [`ConfigError::NotFound`] if a named file is missing, or [`ConfigError::Parse`] if a
    /// layer cannot be merged into a valid [`Config`].
    pub fn load(self) -> Result<Config> {
        let file = match self.file {
            Some(path) if !path.exists() => return Err(ConfigError::NotFound { path }),
            other => other,
        };

        let defaults = Figment::from(Serialized::defaults(Config::default()));
        let with_file = match file.as_deref() {
            Some(path) => defaults.merge(Toml::file(path)),
            None => defaults,
        };
        let with_environment = with_file.merge(Env::prefixed(ENV_PREFIX).split(ENV_SEPARATOR));
        let merged = self
            .overrides
            .iter()
            .fold(with_environment, |figment, config_override| config_override.apply(figment));

        let mut config = extract(&merged, file.as_deref())?;
        config.apply_memory_profile();

        Ok(config)
    }
}

/// Extracts the merged configuration, naming the file in the failure when one was read.
fn extract(figment: &Figment, file: Option<&Path>) -> Result<Config> {
    figment.extract().map_err(|error| match file {
        Some(path) => ConfigError::Parse {
            path: path.to_path_buf(),
            source: Box::new(error),
        },
        None => ConfigError::Assemble { source: Box::new(error) },
    })
}

#[cfg(test)]
mod tests {
    use crate::{loader::ConfigLoader, overrides::ConfigOverride, pipeline::ExtractionMode};
    use cassiopeia_common::{memory_profile::MemoryProfile, store_kind::StoreKind};
    use serial_test::serial;
    use std::{fs::write, path::PathBuf};
    use tempfile::TempDir;

    // Every test here loads through the `Env` provider, which reads the process environment, so
    // they run one at a time rather than observing a variable another test is setting.

    /// Writes a configuration file into a fresh directory and hands back both, so the directory
    /// outlives the path the test loads from.
    fn config_file(content: &str) -> (TempDir, PathBuf) {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let path = directory.path().join("cassiopeia.toml");
        write(&path, content).expect("the configuration file can be written");

        (directory, path)
    }

    #[test]
    #[serial(environment)]
    fn the_built_in_defaults_load_on_their_own() {
        let config = ConfigLoader::new().load().expect("the defaults are a valid configuration");

        assert_eq!(config.pipeline.batch_size.get(), 10_000);
        assert_eq!(config.pipeline.channel_capacity, None);
    }

    #[test]
    #[serial(environment)]
    fn a_missing_configuration_file_is_reported_rather_than_ignored() {
        let directory = TempDir::new().expect("a temporary directory can be created");

        assert!(ConfigLoader::new().with_file(directory.path().join("absent.toml")).load().is_err());
    }

    #[test]
    #[serial(environment)]
    fn the_file_overrides_the_built_in_defaults() {
        let (_directory, path) = config_file("[pipeline]\nbatch_size = 25\n");

        let config = ConfigLoader::new().with_file(path).load().expect("the file is valid");

        assert_eq!(config.pipeline.batch_size.get(), 25);
    }

    #[test]
    #[serial(environment)]
    fn the_environment_overrides_the_file() {
        let (_directory, path) = config_file("[pipeline]\nbatch_size = 25\n");

        let config = temp_env::with_var("CASSIOPEIA_PIPELINE__BATCH_SIZE", Some("40"), || {
            ConfigLoader::new().with_file(path).load().expect("the file is valid")
        });

        assert_eq!(config.pipeline.batch_size.get(), 40);
    }

    #[test]
    #[serial(environment)]
    fn an_override_displaces_only_the_setting_it_names() {
        let (_directory, path) = config_file("[resolver]\nrelationship_store = \"redb\"\n");

        let config = ConfigLoader::new()
            .with_file(path)
            .with_override(ConfigOverride::EntityStore(StoreKind::Redb))
            .load()
            .expect("the file is valid");

        // The override names only the entity store; the relationship store keeps the file's value.
        assert_eq!(config.resolver.entity_store, StoreKind::Redb);
        assert_eq!(config.resolver.relationship_store, StoreKind::Redb);
    }

    #[test]
    #[serial(environment)]
    fn the_low_memory_profile_is_resolved_after_every_layer_has_merged() {
        let (_directory, path) = config_file("[pipeline]\nbatch_size = 50000\n");

        let config = ConfigLoader::new()
            .with_file(path)
            .with_override(ConfigOverride::MemoryProfile(MemoryProfile::LowMemory))
            .load()
            .expect("the file is valid");

        assert_eq!(config.pipeline.batch_size.get(), 2_000);
        assert_eq!(config.pipeline.extraction_mode, ExtractionMode::Sequential);
        assert_eq!(config.resolver.entity_store, StoreKind::Redb);
    }
}

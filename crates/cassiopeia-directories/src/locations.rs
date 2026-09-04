use crate::base::{config_dir, data_dir, state_dir};
use std::path::PathBuf;

/// The leaf name of the schema store within the data directory.
const SCHEMAS_LEAF: &str = "schemas";
/// The leaf name of the mapping folder within the config directory.
const MAPPINGS_LEAF: &str = "mappings";
/// The leaf name of the log directory within the state directory.
const LOG_LEAF: &str = "logs";
/// The name of the configuration file within the config directory.
const CONFIG_FILE_NAME: &str = "config.toml";

/// Where downloaded Smart Data Models JSON Schemas are kept.
///
/// Schemas are downloaded application data, so they belong under the data directory.
#[must_use]
pub fn schemas_dir() -> PathBuf {
    data_dir().join(SCHEMAS_LEAF)
}

/// Where the mapping documents a run refers to by name are kept.
///
/// Mappings are user-authored configuration, so they belong under the config directory.
#[must_use]
pub fn mappings_dir() -> PathBuf {
    config_dir().join(MAPPINGS_LEAF)
}

/// Where the rolling log files are written.
///
/// Logs are volatile run-state, so they belong under the state directory.
#[must_use]
pub fn log_dir() -> PathBuf {
    state_dir().join(LOG_LEAF)
}

/// The path the installation-wide configuration file is discovered at.
///
/// The file lives directly in the config directory so auto-discovery and `config generate` agree
/// on one location.
#[must_use]
pub fn config_file() -> PathBuf {
    config_dir().join(CONFIG_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use crate::{
        base::{config_dir, data_dir},
        locations::{config_file, log_dir, mappings_dir, schemas_dir},
    };

    #[test]
    fn schemas_are_classified_under_the_data_directory() {
        assert!(schemas_dir().starts_with(data_dir()));
        assert!(schemas_dir().ends_with("schemas"));
    }

    #[test]
    fn mappings_are_classified_under_the_config_directory() {
        assert!(mappings_dir().starts_with(config_dir()));
        assert!(mappings_dir().ends_with("mappings"));
    }

    #[test]
    fn the_log_directory_is_named_after_what_it_holds() {
        assert!(log_dir().ends_with("logs"));
    }

    #[test]
    fn the_configuration_file_lives_in_the_config_directory() {
        assert!(config_file().starts_with(config_dir()));
        assert!(config_file().ends_with("config.toml"));
    }
}

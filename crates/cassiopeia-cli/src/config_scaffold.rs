use crate::error::{CliError, IoAction, Result};
use cassiopeia_configuration::config::Config;
use std::{
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
};

/// Whether an existing file at the target may be overwritten.
///
/// A named enum rather than a bare `bool` so the two outcomes read at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Replace an existing file.
    Force,
    /// Refuse to clobber an existing file.
    Keep,
}

impl Overwrite {
    /// Reads the `--force` flag into the overwrite decision.
    #[must_use]
    pub const fn from_flag(force: bool) -> Overwrite {
        if force { Overwrite::Force } else { Overwrite::Keep }
    }
}

/// Writes a fully-populated default configuration file to `path`, returning the path written.
///
/// The file is the serialized [`Config::default`], so it carries every section with its current
/// default value; a later run auto-discovers it. Parent directories are created as needed. An
/// existing file is left untouched unless [`Overwrite::Force`] is given.
///
/// # Errors
/// Returns [`CliError::FileAlreadyExists`] when the target exists and overwriting was not
/// requested, [`CliError::SerializeConfig`] when the defaults cannot be serialized, or
/// [`CliError::FileOperation`] when a directory or the file cannot be written.
pub fn write_default_config(path: &Path, overwrite: Overwrite) -> Result<PathBuf> {
    if path.exists() && overwrite == Overwrite::Keep {
        return Err(CliError::FileAlreadyExists { path: path.to_path_buf() });
    }

    let serialized = toml::to_string_pretty(&Config::default()).map_err(CliError::SerializeConfig)?;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent).map_err(|source| CliError::FileOperation {
            source,
            path: parent.to_path_buf(),
            action: IoAction::Create,
        })?;
    }

    write(path, serialized).map_err(|source| CliError::FileOperation {
        source,
        path: path.to_path_buf(),
        action: IoAction::Write,
    })?;

    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use crate::{
        config_scaffold::{Overwrite, write_default_config},
        error::CliError,
    };
    use cassiopeia_configuration::config::Config;
    use std::fs::{read_to_string, write};
    use tempfile::TempDir;

    #[test]
    fn the_written_file_round_trips_back_to_the_default_configuration() {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let path = directory.path().join("nested").join("config.toml");

        let written = write_default_config(&path, Overwrite::Keep).expect("the default configuration is written");
        assert_eq!(written, path);

        let contents = read_to_string(&path).expect("the written file can be read");
        let parsed: Config = toml::from_str(&contents).expect("the written file parses as configuration");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn an_existing_file_is_not_clobbered_without_force() {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let path = directory.path().join("config.toml");
        write(&path, "keep me").expect("the sentinel file can be written");

        let result = write_default_config(&path, Overwrite::Keep);

        assert!(matches!(result, Err(CliError::FileAlreadyExists { .. })));
        assert_eq!(read_to_string(&path).unwrap(), "keep me");
    }

    #[test]
    fn force_overwrites_an_existing_file() {
        let directory = TempDir::new().expect("a temporary directory can be created");
        let path = directory.path().join("config.toml");
        write(&path, "replace me").expect("the sentinel file can be written");

        write_default_config(&path, Overwrite::Force).expect("the file is overwritten");

        let parsed: Config = toml::from_str(&read_to_string(&path).unwrap()).expect("the overwritten file parses");
        assert_eq!(parsed, Config::default());
    }
}

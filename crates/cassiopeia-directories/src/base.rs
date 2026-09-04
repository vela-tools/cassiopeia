use etcetera::app_strategy::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use is_docker::is_docker;
use std::path::PathBuf;

/// The configuration root a container uses, following the Filesystem Hierarchy Standard.
const CONTAINER_CONFIG_ROOT: &str = "/etc/cassiopeia";
/// The data root a container uses.
const CONTAINER_DATA_ROOT: &str = "/var/lib/cassiopeia";
/// The state root a container uses; logs are state, so they land under `/var/log`.
const CONTAINER_STATE_ROOT: &str = "/var/log/cassiopeia";
/// The application name the XDG project directories are derived from.
const APPLICATION: &str = "cassiopeia";
/// The relative root used when no home directory is known, resolved against the working directory.
const NO_HOME_ROOT: &str = "cassiopeia";

/// Where the process is running, which decides whether the container roots or the XDG bases apply.
///
/// A container has no per-user home to honour and a conventional system layout to fill instead, so
/// the two cases resolve every base differently and are kept apart rather than branched inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Runtime {
    /// Running inside a container: the Filesystem Hierarchy Standard roots apply.
    Container,
    /// Running on a host with a user profile: the XDG base directories apply.
    Host,
}

impl Runtime {
    /// Detects the runtime by probing for the markers a container leaves behind.
    fn detect() -> Runtime {
        if is_docker() { Runtime::Container } else { Runtime::Host }
    }
}

/// The XDG application directories, absent only when no home directory can be determined.
///
/// `choose_app_strategy` follows the command-line convention: the XDG base directories everywhere
/// except Windows, which uses its own known folders. An undeterminable home directory is its only
/// failure mode, which is the same case the callers already fall back on.
fn app_dirs() -> Option<impl AppStrategy> {
    choose_app_strategy(AppStrategyArgs {
        top_level_domain: String::new(),
        author: String::new(),
        app_name: APPLICATION.to_owned(),
    })
    .ok()
}

/// The directory install-wide configuration belongs in.
///
/// A container uses `/etc/cassiopeia`; a host uses the XDG config directory, falling back to a
/// working-directory-relative path when no home is known.
#[must_use]
pub fn config_dir() -> PathBuf {
    config_root(Runtime::detect())
}

/// The directory downloaded application data belongs in.
///
/// A container uses `/var/lib/cassiopeia`; a host uses the XDG data directory, falling back to a
/// working-directory-relative path when no home is known.
#[must_use]
pub fn data_dir() -> PathBuf {
    data_root(Runtime::detect())
}

/// The directory volatile run-state belongs in.
///
/// A container uses `/var/log/cassiopeia`; a host uses the XDG state directory. Not every platform
/// defines a state directory (Windows does not), so it falls back to the data directory, and to a
/// working-directory-relative path when no home is known.
#[must_use]
pub fn state_dir() -> PathBuf {
    state_root(Runtime::detect())
}

/// Resolves the configuration root for a runtime.
fn config_root(runtime: Runtime) -> PathBuf {
    match runtime {
        Runtime::Container => PathBuf::from(CONTAINER_CONFIG_ROOT),
        Runtime::Host => match app_dirs() {
            Some(dirs) => dirs.config_dir(),
            None => PathBuf::from(NO_HOME_ROOT),
        },
    }
}

/// Resolves the data root for a runtime.
fn data_root(runtime: Runtime) -> PathBuf {
    match runtime {
        Runtime::Container => PathBuf::from(CONTAINER_DATA_ROOT),
        Runtime::Host => match app_dirs() {
            Some(dirs) => dirs.data_dir(),
            None => PathBuf::from(NO_HOME_ROOT),
        },
    }
}

/// Resolves the state root for a runtime, folding onto the data root where no state base exists.
fn state_root(runtime: Runtime) -> PathBuf {
    match runtime {
        Runtime::Container => PathBuf::from(CONTAINER_STATE_ROOT),
        Runtime::Host => match app_dirs() {
            Some(dirs) => match dirs.state_dir() {
                Some(state) => state,
                None => dirs.data_dir(),
            },
            None => PathBuf::from(NO_HOME_ROOT),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::base::{Runtime, config_root, data_root, state_root};
    use std::path::PathBuf;

    #[test]
    fn a_container_puts_configuration_under_etc() {
        assert_eq!(config_root(Runtime::Container), PathBuf::from("/etc/cassiopeia"));
    }

    #[test]
    fn a_container_puts_data_under_var_lib() {
        assert_eq!(data_root(Runtime::Container), PathBuf::from("/var/lib/cassiopeia"));
    }

    #[test]
    fn a_container_puts_state_under_var_log() {
        assert_eq!(state_root(Runtime::Container), PathBuf::from("/var/log/cassiopeia"));
    }

    #[test]
    fn a_host_names_each_base_after_the_application() {
        // With a home present the XDG bases all carry the application name; with none present the
        // working-directory fallback is that same name. Either way the leaf is `cassiopeia`.
        assert!(config_root(Runtime::Host).ends_with("cassiopeia"));
        assert!(data_root(Runtime::Host).ends_with("cassiopeia"));
        assert!(state_root(Runtime::Host).ends_with("cassiopeia"));
    }
}

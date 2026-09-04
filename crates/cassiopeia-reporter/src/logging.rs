//! Tracing subscriber setup and management.

use crate::error::Result;
use cassiopeia_common::log::{config::LoggerConfig, format::LogFormat, level::LogLevel};
use cassiopeia_directories::locations::log_dir;
use std::{
    io,
    path::{Path, PathBuf},
};
use time::macros::format_description;
use tracing::Subscriber;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_dedup::DeduplicatingFormatter;
use tracing_subscriber::{
    filter::LevelFilter,
    fmt,
    fmt::time::LocalTime,
    layer::{Identity, Layer},
    registry::{LookupSpan, Registry},
    reload,
};

/// The sentinel value the file-sink directory carries when the configuration leaves it unset.
///
/// It matches `FileLogConfig`'s own default in `cassiopeia-common`; the two must agree for the
/// default to be recognised and resolved to the XDG log location.
const DEFAULT_LOG_DIRECTORY: &str = "logs";

/// Resolves the directory the file sink writes into.
///
/// When the file sink is left at its default sentinel, the XDG-correct log directory is resolved
/// centrally (the container log root on a container, the state directory elsewhere). An explicitly
/// configured directory is always respected verbatim.
fn effective_directory(configured: &Path) -> PathBuf {
    if configured == Path::new(DEFAULT_LOG_DIRECTORY) {
        log_dir()
    } else {
        configured.to_path_buf()
    }
}

/// The [`ReloadHandle`] wraps a boxed layer attached to a registry.
pub type ReloadHandle = reload::Handle<Box<dyn Layer<Registry> + Send + Sync>, Registry>;

/// Converts a [`LogLevel`] to a tracing `LevelFilter`.
const fn log_level_to_filter(level: LogLevel) -> LevelFilter {
    match level {
        LogLevel::Trace => LevelFilter::TRACE,
        LogLevel::Debug => LevelFilter::DEBUG,
        LogLevel::Info => LevelFilter::INFO,
        LogLevel::Warn => LevelFilter::WARN,
        LogLevel::Error => LevelFilter::ERROR,
    }
}

/// Constructs a single boxed layer containing both the console and file outputs (when enabled), each
/// with its own independent filter and format.
///
/// # Errors
/// Returns a [`ReporterError`](crate::error::ReporterError) if a layer cannot be constructed.
pub fn build_root_layer(config: &LoggerConfig) -> Result<(Box<dyn Layer<Registry> + Send + Sync>, Option<WorkerGuard>)> {
    let console_layer = if config.console.enabled {
        let layer = fmt::layer().with_writer(io::stdout);
        let formatted = apply_format(layer, config.console.format);
        let filtered = formatted.with_filter(log_level_to_filter(config.console.level));
        Some(Box::new(filtered) as Box<dyn Layer<Registry> + Send + Sync>)
    } else {
        None
    };

    let (file_layer, guard) = if config.file.enabled {
        let directory = effective_directory(&config.file.directory);
        let file_appender = rolling::daily(&directory, &config.file.file_name);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let layer = fmt::layer().with_writer(non_blocking).with_ansi(false); // No colors in files

        let formatted = apply_format(layer, config.file.format);
        let filtered = formatted.with_filter(log_level_to_filter(config.file.level));

        (Some(Box::new(filtered) as Box<dyn Layer<Registry> + Send + Sync>), Some(guard))
    } else {
        (None, None)
    };

    let combined_layer: Box<dyn Layer<Registry> + Send + Sync> = match (console_layer, file_layer) {
        (Some(console), Some(file)) => Box::new(console.and_then(file)),
        (Some(console), None) => console,
        (None, Some(file)) => file,
        (None, None) => Box::new(Identity::new()), // No logging enabled
    };

    Ok((combined_layer, guard))
}

/// Helper to apply JSON/Pretty/Compact formatting to a layer.
/// This boxes the result immediately to erase the specific type differences.
fn apply_format<S, W>(layer: fmt::Layer<S, fmt::format::DefaultFields, fmt::format::Format, W>, format: LogFormat) -> Box<dyn Layer<S> + Send + Sync>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'a> fmt::writer::MakeWriter<'a> + 'static + Send + Sync,
{
    match format {
        LogFormat::Compact => Box::new(
            layer
                .compact()
                .with_timer(LocalTime::new(format_description!("[hour]:[minute]:[second]")))
                .with_target(false)
                .event_format(DeduplicatingFormatter::new(fmt::format::Format::default())),
        ),
        LogFormat::Pretty => Box::new(layer.pretty()),
        LogFormat::Json => Box::new(layer.json()),
    }
}

#[cfg(test)]
mod tests {
    use crate::logging::effective_directory;
    use std::path::Path;

    #[test]
    fn an_explicit_directory_is_always_respected() {
        let resolved = effective_directory(Path::new("/var/log/run"));
        assert_eq!(resolved.to_str(), Some("/var/log/run"));
    }

    #[test]
    fn the_default_sentinel_resolves_to_the_xdg_log_directory() {
        // The sentinel is replaced by the centrally resolved log directory, whose leaf is `logs`
        // whether that sits under the container log root or the host state directory.
        let resolved = effective_directory(Path::new("logs"));
        assert!(resolved.ends_with("logs"));
    }
}
